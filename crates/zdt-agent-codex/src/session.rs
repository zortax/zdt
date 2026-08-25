//! One live app-server, and the pipes into it.

use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use zdt_agent::ask::Decision;
use zdt_agent::event::AgentEvent;
use zdt_agent::mode::RuntimeMode;
use zdt_agent_harness::rawlog::RawLog;
use zdt_agent_harness::{HarnessError, SessionStart};

use crate::fold::{
    self, Folder, Shared, State, answers_frame, decision_frame, initialize_frame,
    initialized_frame, interrupt_frame, models_frame, open_thread_frame, steer_frame, turn_frame,
};

/// One spawned app-server.
pub struct Live {
    child: Child,
    /// Where frames for the server's stdin go. One writer task drains it in order.
    writes: UnboundedSender<serde_json::Value>,
    /// Whether the reader task has seen the stream end.
    gone: Arc<AtomicBool>,
    /// What the adapter's methods and the reader both touch.
    state: Shared,
    /// Counts this side's request ids, above the folder's fixed ones.
    requests: Arc<AtomicI64>,
}

impl Live {
    /// Spawns the server for `start`, opens its thread, and starts reading it.
    pub fn spawn(
        binary: &str,
        home: &str,
        start: &SessionStart,
        events: UnboundedSender<AgentEvent>,
        logs: Option<&Path>,
    ) -> Result<Self, HarnessError> {
        let mut command = Command::new(binary);
        command
            .arg("app-server")
            .current_dir(&start.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        if !home.is_empty() {
            command.env("CODEX_HOME", home);
        }

        let mut child = command.spawn().map_err(|source| HarnessError::Spawn {
            program: binary.to_owned(),
            source,
        })?;
        let mut stdin = child.stdin.take().expect("stdin was piped");
        let stdout = child.stdout.take().expect("stdout was piped");

        let state: Shared = Arc::new(Mutex::new(State {
            mode: start.mode,
            model: start.model.clone(),
            effort: start.effort.clone(),
            resuming: start.resume.is_some(),
            ..State::default()
        }));

        let (writes, mut outbox) = unbounded_channel::<serde_json::Value>();

        // The handshake, then the thread, then the catalog. The turn itself waits for the
        // thread's answer: the folder writes it once the conversation has a name.
        let _ = writes.send(initialize_frame());
        let _ = writes.send(initialized_frame());
        {
            let opened = state.lock().expect("the state is never poisoned");
            let _ = writes.send(open_thread_frame(
                fold::THREAD_ID,
                start.resume.as_deref(),
                &opened,
            ));
        }
        let _ = writes.send(models_frame());

        // The writer: one task owns stdin, so answers never interleave mid-line.
        tokio::spawn(async move {
            while let Some(value) = outbox.recv().await {
                let mut bytes = serde_json::to_vec(&value).expect("a value encodes");
                bytes.push(b'\n');
                if stdin.write_all(&bytes).await.is_err() || stdin.flush().await.is_err() {
                    return;
                }
            }
        });

        // The reader. One task per session, alive exactly as long as the pipe.
        let gone = Arc::new(AtomicBool::new(false));
        let ended = Arc::clone(&gone);
        let thread = start.thread;
        let mut log = match logs {
            Some(directory) => RawLog::open(&directory.join(format!("thread-{thread}.ndjson"))),
            None => RawLog::nowhere(),
        };
        let answering = writes.clone();
        let folding = Arc::clone(&state);
        tokio::spawn(async move {
            let mut folder = Folder::new(thread, folding);
            zdt_agent_harness::ndjson::each_value(stdout, |value| {
                log.line(&value);
                let fold = folder.take(&value);
                for frame in fold.writes {
                    let _ = answering.send(frame);
                }
                for event in fold.events {
                    let _ = events.send(event);
                }
            })
            .await;
            ended.store(true, Ordering::Release);
            for event in folder.ended() {
                let _ = events.send(event);
            }
        });

        Ok(Self {
            child,
            writes,
            gone,
            state,
            requests: Arc::new(AtomicI64::new(0)),
        })
    }

    /// Whether the process has gone.
    #[must_use]
    pub fn is_gone(&self) -> bool {
        self.gone.load(Ordering::Acquire)
    }

    /// Writes one prompt: a turn when the thread is idle, a steer into the turn that runs, and
    /// a queued start while the thread is still coming up.
    pub fn say(&mut self, text: &str) -> Result<(), HarnessError> {
        let frame = {
            let mut state = self.state.lock().expect("the state is never poisoned");
            match (state.session.clone(), state.turn.clone()) {
                (Some(session), Some(turn)) => {
                    Some(steer_frame(self.name(), &session, &turn, text))
                }
                (Some(session), None) => {
                    state.turn_open = true;
                    Some(turn_frame(self.name(), &session, text, &state))
                }
                (None, _) => {
                    state.turn_open = true;
                    state.queued = Some(text.to_owned());
                    None
                }
            }
        };
        match frame {
            Some(frame) => self.write(frame),
            None => Ok(()),
        }
    }

    /// Asks the server to stop the running turn. Nothing runs, nothing to stop.
    pub fn interrupt(&mut self) -> Result<(), HarnessError> {
        let frame = {
            let state = self.state.lock().expect("the state is never poisoned");
            match (&state.session, &state.turn) {
                (Some(session), Some(turn)) => Some(interrupt_frame(self.name(), session, turn)),
                _ => None,
            }
        };
        match frame {
            Some(frame) => self.write(frame),
            None => Ok(()),
        }
    }

    /// Answers a pending approval ask. `false` when the ask is not open here.
    pub fn decide(&mut self, ask: &str, decision: &Decision) -> Result<bool, HarnessError> {
        let Some(open) = self
            .state
            .lock()
            .expect("the state is never poisoned")
            .asks
            .remove(ask)
        else {
            return Ok(false);
        };
        self.write(decision_frame(&open.rpc_id, decision))?;
        Ok(true)
    }

    /// Answers a pending question ask. `false` when the ask is not open here.
    pub fn answer(&mut self, ask: &str, answers: &[Vec<String>]) -> Result<bool, HarnessError> {
        let Some(open) = self
            .state
            .lock()
            .expect("the state is never poisoned")
            .asks
            .remove(ask)
        else {
            return Ok(false);
        };
        self.write(answers_frame(&open.rpc_id, &open.question_ids, answers))?;
        Ok(true)
    }

    /// Moves the session to `mode`. The next turn start carries it.
    pub fn set_mode(&mut self, mode: RuntimeMode) {
        self.state.lock().expect("the state is never poisoned").mode = mode;
    }

    /// Moves the session to `model`. The next turn start carries it.
    pub fn set_model(&mut self, model: &str) {
        self.state
            .lock()
            .expect("the state is never poisoned")
            .model = model.to_owned();
    }

    /// Moves the session to `effort`. The next turn start carries it.
    pub fn set_effort(&mut self, effort: &str) {
        self.state
            .lock()
            .expect("the state is never poisoned")
            .effort = effort.to_owned();
    }

    /// Stops the process.
    pub async fn kill(&mut self) {
        let _ = self.child.kill().await;
    }

    /// A request id of this side's own, above the folder's.
    fn name(&self) -> i64 {
        fold::DYNAMIC_IDS + 1000 + self.requests.fetch_add(1, Ordering::Relaxed)
    }

    fn write(&mut self, value: serde_json::Value) -> Result<(), HarnessError> {
        self.writes
            .send(value)
            .map_err(|_| HarnessError::Gone("the writer is gone".to_owned()))
    }
}
