//! The adapter seam.
//!
//! A provider adapter wraps one agent harness — a process, a protocol — and shows the daemon
//! one face: commands in, [`AgentEvent`]s out. Everything a harness is peculiar about stays
//! inside its adapter, which is what makes a second harness a second crate rather than a second
//! daemon.
//!
//! [`AgentEvent`]: zdt_agent::event::AgentEvent

pub mod conformance;
pub mod ndjson;
pub mod rawlog;

use std::path::PathBuf;

use zdt_agent::ask::Decision;
use zdt_agent::mode::RuntimeMode;
use zdt_agent::thread::ThreadId;

/// One provider-side conversation an adapter found on disk, offered for import.
#[derive(Clone, Debug)]
pub struct FoundImport {
    /// The provider's own name for it: the resume cursor an imported thread starts with.
    pub id: String,
    /// What to call it: the provider's own title, or its first prompt.
    pub title: String,
    /// The directory it worked in.
    pub cwd: PathBuf,
    /// When it last moved, in milliseconds since the epoch.
    pub at_ms: u64,
}

/// One provider-side conversation, read whole for import.
#[derive(Clone, Debug)]
pub struct SessionDump {
    /// The provider's own name for it.
    pub id: String,
    /// What to call it.
    pub title: String,
    /// The directory it worked in.
    pub cwd: PathBuf,
    /// The prose, oldest first. Tool runs are not carried over.
    pub lines: Vec<DumpLine>,
}

/// One message of an imported conversation.
#[derive(Clone, Debug)]
pub struct DumpLine {
    /// Whether the person said it; the agent otherwise.
    pub user: bool,
    /// What was said.
    pub text: String,
}

/// What starting a provider session needs.
#[derive(Clone, Debug)]
pub struct SessionStart {
    /// Which thread the session serves.
    pub thread: ThreadId,
    /// The directory the agent works in.
    pub cwd: PathBuf,
    /// The provider's own name for the conversation, from an earlier session.
    ///
    /// Opaque above the adapter: the daemon stores it and hands it back, and only the adapter
    /// knows its shape.
    pub resume: Option<String>,
    /// Which model, in the provider's own words. Empty means the provider's default.
    pub model: String,
    /// How hard the agent reasons, in the provider's own words. Empty means its default.
    pub effort: String,
    /// How much the agent may do unasked.
    pub mode: RuntimeMode,
}

/// What went wrong inside an adapter.
#[derive(Debug, thiserror::Error)]
pub enum HarnessError {
    /// The provider's program could not be started.
    #[error("could not start {program}: {source}")]
    Spawn {
        /// What was run.
        program: String,
        /// What the system said.
        #[source]
        source: std::io::Error,
    },
    /// The provider's process is gone or its pipe broke.
    #[error("the provider went away: {0}")]
    Gone(String),
    /// The thread has no live session and nothing to resume.
    #[error("no session for thread {0}")]
    NoSession(ThreadId),
}

/// One harness, as the daemon drives it.
///
/// Sessions are the adapter's own: `send_turn` starts one when the thread has none, resuming
/// from [`SessionStart::resume`] when it can. The adapter's only output is the event channel it
/// was built with.
pub trait ProviderAdapter {
    /// Which harness this is, for the log and the database.
    fn kind(&self) -> &'static str;

    /// Sends a prompt, starting or resuming the thread's session first when it has to.
    fn send_turn(
        &self,
        start: SessionStart,
        text: String,
    ) -> impl Future<Output = Result<(), HarnessError>>;

    /// Stops the turn that is running, leaving the session alive.
    fn interrupt(&self, thread: ThreadId) -> impl Future<Output = Result<(), HarnessError>>;

    /// Answers an open tool ask.
    fn decide(
        &self,
        thread: ThreadId,
        id: String,
        decision: Decision,
    ) -> impl Future<Output = Result<(), HarnessError>>;

    /// Answers an open question ask with the chosen option labels, one list per question.
    fn answer(
        &self,
        thread: ThreadId,
        id: String,
        answers: Vec<Vec<String>>,
    ) -> impl Future<Output = Result<(), HarnessError>>;

    /// Moves a live session to `mode`. A thread with no live session takes it at the next spawn.
    fn set_mode(
        &self,
        thread: ThreadId,
        mode: RuntimeMode,
    ) -> impl Future<Output = Result<(), HarnessError>>;

    /// Moves a live session to `model`. A thread with no live session takes it at the next spawn.
    fn set_model(
        &self,
        thread: ThreadId,
        model: String,
    ) -> impl Future<Output = Result<(), HarnessError>>;

    /// Stops the thread's session.
    fn stop(&self, thread: ThreadId) -> impl Future<Output = ()>;

    /// Stops every session, which is what the daemon shutting down does.
    fn stop_all(&self) -> impl Future<Output = ()>;
}
