//! Claude Code, spoken to directly.
//!
//! The `claude` CLI carries a newline-delimited JSON protocol on its pipes when started with
//! `--input-format stream-json --output-format stream-json`. This adapter spawns one CLI per
//! thread, writes user messages and control requests onto stdin, and folds what comes back into
//! [`AgentEvent`]s. No SDK sits in between: the wire is the contract, and the captured
//! transcripts under `tests/fixtures` are what pin it down.
//!
//! # Sessions
//!
//! The CLI stays alive between turns while its stdin is open, and names its conversation with a
//! session id this adapter chooses. The id is the resume cursor: a thread whose process is gone
//! starts a new one with `--resume` and the conversation carries on.
//!
//! [`AgentEvent`]: zdt_agent::event::AgentEvent

mod fold;
pub mod import;
mod session;
mod skills;
mod tasks;
mod tools;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;
use tokio::sync::mpsc::UnboundedSender;
use zdt_agent::ask::Decision;
use zdt_agent::event::AgentEvent;
use zdt_agent::mode::RuntimeMode;
use zdt_agent::thread::ThreadId;
use zdt_agent_harness::{HarnessError, ProviderAdapter, SessionStart};

use crate::session::Live;

/// The Claude Code harness.
///
/// Cloning one is cloning a handle: every clone drives the same sessions.
#[derive(Clone)]
pub struct ClaudeAdapter {
    inner: Arc<Inner>,
}

struct Inner {
    /// Where everything noticed goes.
    events: UnboundedSender<AgentEvent>,
    /// One live CLI per thread.
    sessions: Mutex<HashMap<ThreadId, Live>>,
    /// The program to run.
    binary: String,
    /// The `CLAUDE_CONFIG_DIR` sessions run under. Empty means the CLI's own default, and a
    /// directory of its own is a whole account of its own.
    home: String,
    /// Where raw transcripts are appended, when anywhere.
    logs: Option<PathBuf>,
}

impl ClaudeAdapter {
    /// An adapter with no sessions, reporting into `events`.
    ///
    /// `binary` is the program to run; empty means `claude` off the search path. `home` is the
    /// `CLAUDE_CONFIG_DIR` sessions run under; empty leaves the CLI its default. `logs` is a
    /// directory for raw NDJSON transcripts, one file per thread.
    #[must_use]
    pub fn new(
        events: UnboundedSender<AgentEvent>,
        binary: String,
        home: String,
        logs: Option<PathBuf>,
    ) -> Self {
        let binary = if binary.is_empty() {
            "claude".to_owned()
        } else {
            binary
        };
        Self {
            inner: Arc::new(Inner {
                events,
                sessions: Mutex::new(HashMap::new()),
                binary,
                home,
                logs,
            }),
        }
    }
}

impl ClaudeAdapter {
    /// Learns what a session in `cwd` would offer, without running one for real.
    ///
    /// A short-lived CLI is spawned, its initialize answer flows out as a catalog event through
    /// the ordinary channel, and the process is stopped. Nothing happens when the thread already
    /// has a live session: its own initialize has answered.
    pub fn probe(&self, thread: ThreadId, cwd: PathBuf) {
        let adapter = self.clone();
        tokio::spawn(async move {
            {
                let sessions = adapter.inner.sessions.lock().await;
                if sessions.contains_key(&thread) {
                    return;
                }
            }
            // The skills on disk, said first: the probe's answer has no skills field, and the
            // init message that does only comes with a real turn.
            let skills = crate::skills::discover(&cwd);
            if !skills.is_empty() {
                let _ = adapter.inner.events.send(AgentEvent::Catalog {
                    thread,
                    catalog: zdt_agent::catalog::Catalog {
                        skills,
                        ..zdt_agent::catalog::Catalog::default()
                    },
                });
            }
            let start = SessionStart {
                thread,
                cwd,
                resume: None,
                model: String::new(),
                effort: String::new(),
                mode: RuntimeMode::Supervised,
            };
            // Only the catalog leaves a probe. Its session events must not reach the daemon:
            // a probe's throwaway conversation stored as the thread's resume cursor would lose
            // the real one.
            let (catalog_only, mut sifted) = tokio::sync::mpsc::unbounded_channel();
            {
                let events = adapter.inner.events.clone();
                tokio::spawn(async move {
                    while let Some(event) = sifted.recv().await {
                        if matches!(event, AgentEvent::Catalog { .. })
                            && events.send(event).is_err()
                        {
                            return;
                        }
                    }
                });
            }
            let Ok(mut live) = Live::spawn(
                &adapter.inner.binary,
                &adapter.inner.home,
                &start,
                catalog_only,
                None,
            ) else {
                return;
            };
            // Long enough for the initialize answer, short enough not to linger.
            tokio::time::sleep(std::time::Duration::from_secs(8)).await;
            live.kill().await;
        });
    }
}

impl ClaudeAdapter {
    /// The conversations the CLI already holds, offered for import. Blocking file reads.
    #[must_use]
    pub fn importable(&self) -> Vec<zdt_agent_harness::FoundImport> {
        crate::import::list(&self.inner.home)
    }

    /// One of them, read whole. Blocking file reads.
    #[must_use]
    pub fn import_dump(&self, id: &str) -> Option<zdt_agent_harness::SessionDump> {
        crate::import::read(&self.inner.home, id)
    }

    /// Asks the CLI one question outside any session and answers its plain-text reply.
    ///
    /// What title generation runs on: `--print` with a cheap model, no session kept. Empty
    /// `model` means `haiku`. Nothing on any failure — a name is a convenience.
    pub async fn oneshot(&self, model: &str, prompt: &str) -> Option<String> {
        use tokio::io::AsyncWriteExt;
        let model = if model.is_empty() { "haiku" } else { model };
        let mut command = tokio::process::Command::new(&self.inner.binary);
        command
            .args(["--print", "--model", model])
            .current_dir(std::env::temp_dir())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        if !self.inner.home.is_empty() {
            command.env("CLAUDE_CONFIG_DIR", &self.inner.home);
        }
        let mut child = command.spawn().ok()?;
        let mut stdin = child.stdin.take()?;
        stdin.write_all(prompt.as_bytes()).await.ok()?;
        drop(stdin);
        let out =
            tokio::time::timeout(std::time::Duration::from_secs(90), child.wait_with_output())
                .await
                .ok()?
                .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
            .filter(|said| !said.is_empty())
    }
}

impl ProviderAdapter for ClaudeAdapter {
    fn kind(&self) -> &'static str {
        "claude"
    }

    async fn send_turn(&self, start: SessionStart, text: String) -> Result<(), HarnessError> {
        let mut sessions = self.inner.sessions.lock().await;
        let thread = start.thread;

        // A session whose process has gone is swept before anything is written to it. So is one
        // spawned under another effort: the flag only rides a spawn, and the fresh process
        // resumes the conversation by name.
        let stale = sessions
            .get(&thread)
            .is_some_and(|live| live.is_gone() || live.effort != start.effort);
        if stale && let Some(mut live) = sessions.remove(&thread) {
            live.kill().await;
        }
        if let std::collections::hash_map::Entry::Vacant(place) = sessions.entry(thread) {
            place.insert(Live::spawn(
                &self.inner.binary,
                &self.inner.home,
                &start,
                self.inner.events.clone(),
                self.inner.logs.as_deref(),
            )?);
        }
        let live = sessions.get_mut(&thread).expect("just inserted");
        let sent = live.say(&text);
        if sent.is_err() {
            sessions.remove(&thread);
        }
        sent
    }

    async fn interrupt(&self, thread: ThreadId) -> Result<(), HarnessError> {
        let mut sessions = self.inner.sessions.lock().await;
        let Some(live) = sessions.get_mut(&thread) else {
            return Err(HarnessError::NoSession(thread));
        };
        if live.interrupt().is_err() {
            // A session that will not take a control request is stopped the hard way.
            if let Some(mut live) = sessions.remove(&thread) {
                live.kill().await;
            }
        }
        Ok(())
    }

    async fn decide(
        &self,
        thread: ThreadId,
        id: String,
        decision: Decision,
    ) -> Result<(), HarnessError> {
        let mut sessions = self.inner.sessions.lock().await;
        let Some(live) = sessions.get_mut(&thread) else {
            return Err(HarnessError::NoSession(thread));
        };
        if !live.decide(&id, &decision)? {
            return Err(HarnessError::Gone("that ask is no longer open".to_owned()));
        }
        Ok(())
    }

    async fn answer(
        &self,
        thread: ThreadId,
        id: String,
        answers: Vec<Vec<String>>,
    ) -> Result<(), HarnessError> {
        let mut sessions = self.inner.sessions.lock().await;
        let Some(live) = sessions.get_mut(&thread) else {
            return Err(HarnessError::NoSession(thread));
        };
        if !live.answer(&id, &answers)? {
            return Err(HarnessError::Gone("that ask is no longer open".to_owned()));
        }
        Ok(())
    }

    async fn set_mode(&self, thread: ThreadId, mode: RuntimeMode) -> Result<(), HarnessError> {
        let mut sessions = self.inner.sessions.lock().await;
        // A thread with no live session takes the mode at its next spawn.
        let Some(live) = sessions.get_mut(&thread) else {
            return Ok(());
        };
        live.set_mode(mode)
    }

    async fn set_model(&self, thread: ThreadId, model: String) -> Result<(), HarnessError> {
        let mut sessions = self.inner.sessions.lock().await;
        // A thread with no live session takes the model at its next spawn.
        let Some(live) = sessions.get_mut(&thread) else {
            return Ok(());
        };
        live.set_model(&model)
    }

    async fn stop(&self, thread: ThreadId) {
        if let Some(mut live) = self.inner.sessions.lock().await.remove(&thread) {
            live.kill().await;
        }
    }

    async fn stop_all(&self) {
        let drained: Vec<Live> = {
            let mut sessions = self.inner.sessions.lock().await;
            sessions.drain().map(|(_, live)| live).collect()
        };
        for mut live in drained {
            live.kill().await;
        }
    }
}
