//! Codex, spoken to through its app server.
//!
//! `codex app-server` carries newline-delimited JSON-RPC on its pipes. This adapter spawns one
//! server per thread, opens the conversation with `thread/start` or `thread/resume`, and folds
//! the notification stream into [`AgentEvent`]s. The wire is the contract, and the captured
//! transcripts under `tests/fixtures` are what pin it down.
//!
//! # Sessions
//!
//! The server names its conversation with a thread id of its own, which is the resume cursor: a
//! thread whose process is gone starts a new server, resumes by that id, and carries on. The
//! rollouts live under `CODEX_HOME`, so a home of its own is an account of its own.
//!
//! [`AgentEvent`]: zdt_agent::event::AgentEvent

pub mod fold;
pub mod import;
mod session;

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

/// The Codex harness.
///
/// Cloning one is cloning a handle: every clone drives the same sessions.
#[derive(Clone)]
pub struct CodexAdapter {
    inner: Arc<Inner>,
}

struct Inner {
    /// Where everything noticed goes.
    events: UnboundedSender<AgentEvent>,
    /// One live server per thread.
    sessions: Mutex<HashMap<ThreadId, Live>>,
    /// The program to run.
    binary: String,
    /// The `CODEX_HOME` sessions run under. Empty means the CLI's own default.
    home: String,
    /// Where raw transcripts are appended, when anywhere.
    logs: Option<PathBuf>,
}

impl CodexAdapter {
    /// An adapter with no sessions, reporting into `events`.
    ///
    /// `binary` is the program to run; empty means `codex` off the search path. `home` is the
    /// `CODEX_HOME` sessions run under; empty leaves the CLI its default. `logs` is a directory
    /// for raw NDJSON transcripts, one file per thread.
    #[must_use]
    pub fn new(
        events: UnboundedSender<AgentEvent>,
        binary: String,
        home: String,
        logs: Option<PathBuf>,
    ) -> Self {
        let binary = if binary.is_empty() {
            "codex".to_owned()
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

    /// Learns what a session in `cwd` would offer, without running one for real.
    ///
    /// A short-lived server is spawned, its model list flows out as a catalog event through the
    /// ordinary channel, and the process is stopped. Nothing happens when the thread already has
    /// a live session: its own list has answered.
    pub fn probe(&self, thread: ThreadId, cwd: PathBuf) {
        let adapter = self.clone();
        tokio::spawn(async move {
            {
                let sessions = adapter.inner.sessions.lock().await;
                if sessions.contains_key(&thread) {
                    return;
                }
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
            // Long enough for the model list, short enough not to linger.
            tokio::time::sleep(std::time::Duration::from_secs(8)).await;
            live.kill().await;
        });
    }
}

impl CodexAdapter {
    /// The rollouts the CLI already holds, offered for import. Blocking file reads.
    #[must_use]
    pub fn importable(&self) -> Vec<zdt_agent_harness::FoundImport> {
        crate::import::list(&self.inner.home)
    }

    /// One of them, read whole. Blocking file reads.
    #[must_use]
    pub fn import_dump(&self, id: &str) -> Option<zdt_agent_harness::SessionDump> {
        crate::import::read(&self.inner.home, id)
    }

    /// Asks the CLI one question outside any session and answers its final message.
    ///
    /// What title generation runs on: `codex exec --ephemeral`, read-only, no rollout kept. The
    /// answer comes through `--output-last-message`, apart from the run's own narration. Empty
    /// `model` means the provider's default. Nothing on any failure — a name is a convenience.
    pub async fn oneshot(&self, model: &str, prompt: &str) -> Option<String> {
        use tokio::io::AsyncWriteExt;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let answer = std::env::temp_dir().join(format!("zdt-codex-oneshot-{stamp}.txt"));
        let mut command = tokio::process::Command::new(&self.inner.binary);
        command
            .args([
                "exec",
                "--ephemeral",
                "--skip-git-repo-check",
                "-s",
                "read-only",
                "--color",
                "never",
            ])
            .arg("-C")
            .arg(std::env::temp_dir())
            .arg("--output-last-message")
            .arg(&answer)
            .current_dir(std::env::temp_dir())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        if !model.is_empty() {
            command.args(["-m", model]);
        }
        if !self.inner.home.is_empty() {
            command.env("CODEX_HOME", &self.inner.home);
        }
        command.arg("-");
        let mut child = command.spawn().ok()?;
        let mut stdin = child.stdin.take()?;
        stdin.write_all(prompt.as_bytes()).await.ok()?;
        drop(stdin);
        let done = tokio::time::timeout(std::time::Duration::from_secs(90), child.wait())
            .await
            .ok()?
            .ok()?;
        let said = std::fs::read_to_string(&answer).ok();
        let _ = std::fs::remove_file(&answer);
        done.success()
            .then_some(said)?
            .map(|said| said.trim().to_owned())
            .filter(|said| !said.is_empty())
    }
}

impl ProviderAdapter for CodexAdapter {
    fn kind(&self) -> &'static str {
        "codex"
    }

    async fn send_turn(&self, start: SessionStart, text: String) -> Result<(), HarnessError> {
        let mut sessions = self.inner.sessions.lock().await;
        let thread = start.thread;

        // A session whose process has gone is swept before anything is written to it.
        if sessions.get(&thread).is_some_and(Live::is_gone) {
            sessions.remove(&thread);
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
        // A session held between turns takes the thread's latest mode, model and effort with
        // the turn.
        live.set_mode(start.mode);
        live.set_model(&start.model);
        live.set_effort(&start.effort);
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
            // A session that will not take the request is stopped the hard way.
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
        if let Some(live) = sessions.get_mut(&thread) {
            live.set_mode(mode);
        }
        Ok(())
    }

    async fn set_model(&self, thread: ThreadId, model: String) -> Result<(), HarnessError> {
        let mut sessions = self.inner.sessions.lock().await;
        // A thread with no live session takes the model at its next spawn.
        if let Some(live) = sessions.get_mut(&thread) {
            live.set_model(&model);
        }
        Ok(())
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
