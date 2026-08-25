//! One live CLI, and the pipes into it.

use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use zdt_agent::ask::Decision;
use zdt_agent::event::AgentEvent;
use zdt_agent::mode::RuntimeMode;
use zdt_agent_harness::rawlog::RawLog;
use zdt_agent_harness::{HarnessError, SessionStart};

use crate::fold::{Folder, Pending, allow_frame, answer_frame, deny_ask_frame};

/// One spawned CLI.
pub struct Live {
    child: Child,
    /// The effort the process was spawned with. A different one asks for a fresh process.
    pub effort: String,
    /// Where frames for the CLI's stdin go. One writer task drains it in order.
    writes: UnboundedSender<serde_json::Value>,
    /// Whether the reader task has seen the stream end.
    gone: Arc<AtomicBool>,
    /// Tells the folder a prompt was written, so an ended stream is a broken turn.
    turn_started: UnboundedSender<()>,
    /// The permission asks the CLI waits on, shared with the reader.
    pending: Pending,
    /// Counts control requests, so each has a name of its own.
    requests: Arc<AtomicU64>,
}

impl Live {
    /// Spawns the CLI for `start` and starts reading it.
    pub fn spawn(
        binary: &str,
        home: &str,
        start: &SessionStart,
        events: UnboundedSender<AgentEvent>,
        logs: Option<&Path>,
    ) -> Result<Self, HarnessError> {
        let mut command = Command::new(binary);
        if !home.is_empty() {
            command.env("CLAUDE_CONFIG_DIR", home);
        }
        command
            .current_dir(&start.cwd)
            .args([
                "--print",
                "--verbose",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--include-partial-messages",
                // Permission questions come up the pipe as control requests.
                "--permission-prompt-tool",
                "stdio",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        match start.mode {
            // Said outright: an omitted flag would let a settings file choose the mode.
            RuntimeMode::Supervised | RuntimeMode::Unknown => {
                command.args(["--permission-mode", "default"]);
            }
            RuntimeMode::AcceptEdits => {
                command.args(["--permission-mode", "acceptEdits"]);
            }
            RuntimeMode::Auto => {
                command.args(["--permission-mode", "auto"]);
            }
            RuntimeMode::Full => {
                command.args([
                    "--permission-mode",
                    "bypassPermissions",
                    "--dangerously-skip-permissions",
                ]);
            }
            RuntimeMode::Plan => {
                command.args(["--permission-mode", "plan"]);
            }
        }

        // The conversation's name. A thread that has one resumes it; a thread that has none is
        // given one here, so the name is known before any message flows.
        match &start.resume {
            Some(session) => command.args(["--resume", session]),
            None => command.args(["--session-id", &uuid::Uuid::new_v4().to_string()]),
        };
        if !start.model.is_empty() {
            command.args(["--model", &start.model]);
        }
        // A session flag: a change between turns means a fresh process, resumed by name.
        if !start.effort.is_empty() {
            command.args(["--effort", &start.effort]);
        }

        let mut child = command.spawn().map_err(|source| HarnessError::Spawn {
            program: binary.to_owned(),
            source,
        })?;
        let mut stdin = child.stdin.take().expect("stdin was piped");
        let stdout = child.stdout.take().expect("stdout was piped");

        let gone = Arc::new(AtomicBool::new(false));
        let (turn_started, mut turns) = unbounded_channel::<()>();
        let (writes, mut outbox) = unbounded_channel::<serde_json::Value>();
        let pending = Pending::default();

        // Asked first thing: the answer names the commands and the models the session offers.
        // The progress option turns on the `task_progress` line for subagents, which is what
        // carries their token counts and a workflow's phase picture.
        let _ = writes.send(serde_json::json!({
            "type": "control_request",
            "request_id": crate::fold::INITIALIZE_ID,
            "request": {
                "subtype": "initialize",
                "options": { "agentProgressSummaries": true },
            },
        }));

        // The writer: one task owns stdin, so a person's answer and the folder's automatic one
        // never interleave mid-line.
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
        let ended = Arc::clone(&gone);
        let thread = start.thread;
        let mut log = match logs {
            Some(directory) => RawLog::open(&directory.join(format!("thread-{thread}.ndjson"))),
            None => RawLog::nowhere(),
        };
        let answering = writes.clone();
        let asks = Arc::clone(&pending);
        tokio::spawn(async move {
            let mut folder = Folder::new(thread, asks);
            zdt_agent_harness::ndjson::each_value(stdout, |value| {
                // Prompts written since the last line move the folder first, so a result that
                // races the notice still closes the right turn.
                while turns.try_recv().is_ok() {
                    folder.turn_started();
                }
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
            while turns.try_recv().is_ok() {
                folder.turn_started();
            }
            for event in folder.ended() {
                let _ = events.send(event);
            }
        });

        Ok(Self {
            child,
            effort: start.effort.clone(),
            writes,
            gone,
            turn_started,
            pending,
            requests: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Whether the process has gone.
    #[must_use]
    pub fn is_gone(&self) -> bool {
        self.gone.load(Ordering::Acquire)
    }

    /// Writes one user message.
    pub fn say(&mut self, text: &str) -> Result<(), HarnessError> {
        let frame = serde_json::json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": [{ "type": "text", "text": text }],
            },
        });
        let _ = self.turn_started.send(());
        self.write(frame)
    }

    /// Asks the CLI to stop the running turn.
    pub fn interrupt(&mut self) -> Result<(), HarnessError> {
        let frame = self.control(serde_json::json!({ "subtype": "interrupt" }));
        self.write(frame)
    }

    /// Answers a pending permission ask. `false` when the ask is not open here.
    pub fn decide(&mut self, request_id: &str, decision: &Decision) -> Result<bool, HarnessError> {
        let Some(ask) = self
            .pending
            .lock()
            .expect("the ask map is never poisoned")
            .remove(request_id)
        else {
            return Ok(false);
        };
        let frame = match decision {
            Decision::Allow => allow_frame(request_id, &ask, false),
            Decision::AllowAlways => allow_frame(request_id, &ask, true),
            Decision::Deny | Decision::Unknown => deny_ask_frame(request_id, &ask),
        };
        self.write(frame)?;
        Ok(true)
    }

    /// Answers a pending question ask. `false` when the ask is not open here.
    pub fn answer(
        &mut self,
        request_id: &str,
        answers: &[Vec<String>],
    ) -> Result<bool, HarnessError> {
        let Some(ask) = self
            .pending
            .lock()
            .expect("the ask map is never poisoned")
            .remove(request_id)
        else {
            return Ok(false);
        };
        self.write(answer_frame(request_id, &ask, answers))?;
        Ok(true)
    }

    /// Moves the live session to `mode`.
    pub fn set_mode(&mut self, mode: RuntimeMode) -> Result<(), HarnessError> {
        let word = match mode {
            RuntimeMode::Supervised | RuntimeMode::Unknown => "default",
            RuntimeMode::AcceptEdits => "acceptEdits",
            RuntimeMode::Auto => "auto",
            RuntimeMode::Full => "bypassPermissions",
            RuntimeMode::Plan => "plan",
        };
        let frame = self.control(serde_json::json!({
            "subtype": "set_permission_mode",
            "mode": word,
        }));
        self.write(frame)
    }

    /// Moves the live session to `model`.
    pub fn set_model(&mut self, model: &str) -> Result<(), HarnessError> {
        let frame = if model.is_empty() {
            self.control(serde_json::json!({ "subtype": "set_model" }))
        } else {
            self.control(serde_json::json!({ "subtype": "set_model", "model": model }))
        };
        self.write(frame)
    }

    /// Stops the process.
    pub async fn kill(&mut self) {
        let _ = self.child.kill().await;
    }

    /// One control request, with a name of its own.
    fn control(&self, request: serde_json::Value) -> serde_json::Value {
        let name = self.requests.fetch_add(1, Ordering::Relaxed) + 1;
        serde_json::json!({
            "type": "control_request",
            "request_id": format!("zdt-{name}"),
            "request": request,
        })
    }

    fn write(&mut self, value: serde_json::Value) -> Result<(), HarnessError> {
        self.writes
            .send(value)
            .map_err(|_| HarnessError::Gone("the writer is gone".to_owned()))
    }
}
