//! What an adapter reports.
//!
//! The one output a provider adapter has. Everything provider-specific — message shapes,
//! permission grammar, session ids — is translated into this before it leaves the adapter, so
//! the daemon and the editor never learn what a harness looks like inside.

use serde::{Deserialize, Serialize};

use crate::ask::Ask;
use crate::catalog::Catalog;
use crate::thread::{ItemKind, ItemStatus, ThreadId, ToolKind};
use crate::todo::Todo;

/// Which stream a delta belongs to.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamKind {
    /// The answer itself.
    Assistant,
    /// The thinking before it.
    Thinking,
}

/// Whether the provider's process is doing anything.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    /// Coming up.
    Starting,
    /// A turn is running.
    Running,
    /// Alive, with nothing to do.
    Idle,
    /// The process has gone.
    Stopped,
}

/// One tool or task row, as the adapter sees it move.
///
/// Keyed by the provider's own id for the call: the same key arriving again is the same row
/// moving, and the daemon holds the mapping onto timeline rows.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkItem {
    /// The provider's name for the call.
    pub key: String,
    /// Tool or task.
    pub kind: ItemKind,
    /// The tool's name, or the task's description.
    pub name: String,
    /// What sort of tool it is.
    pub tool: ToolKind,
    /// One line saying what it does.
    pub summary: String,
    /// Where it stands.
    pub status: ItemStatus,
    /// What came back, when it is done.
    pub detail: String,
}

/// One thing an adapter noticed.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AgentEvent {
    /// The provider named its session, which is what a resume needs later.
    SessionStarted {
        /// Which thread.
        thread: ThreadId,
        /// The provider's own name for the conversation.
        session: String,
        /// The model the session came up with.
        model: String,
    },
    /// The session said some of what it offers. Empty fields mean "not said here".
    Catalog {
        /// Which thread.
        thread: ThreadId,
        /// What it said.
        catalog: Catalog,
    },
    /// The process moved between doing something and doing nothing.
    State {
        /// Which thread.
        thread: ThreadId,
        /// What it is doing.
        activity: Activity,
    },
    /// A piece of streamed text.
    Delta {
        /// Which thread.
        thread: ThreadId,
        /// Which stream.
        kind: StreamKind,
        /// The piece.
        text: String,
    },
    /// A tool or task moved. The same key again is the same row moving.
    Work {
        /// Which thread.
        thread: ThreadId,
        /// The row.
        item: WorkItem,
    },
    /// The set of runners working beside the main agent changed. The whole set, replacing the
    /// last one.
    Runners {
        /// Which thread.
        thread: ThreadId,
        /// Everything running now.
        runners: Vec<crate::runner::Runner>,
    },
    /// The turn stopped to ask something.
    Asked {
        /// Which thread.
        thread: ThreadId,
        /// The ask.
        ask: Ask,
    },
    /// The provider withdrew an ask before anyone decided.
    AskGone {
        /// Which thread.
        thread: ThreadId,
        /// Which ask.
        id: String,
    },
    /// The provider proposed a plan and waits for a person to take it.
    PlanProposed {
        /// Which thread.
        thread: ThreadId,
        /// The plan, as markdown.
        markdown: String,
    },
    /// The turn's checklist moved.
    Todos {
        /// Which thread.
        thread: ThreadId,
        /// The whole list, replacing the last one.
        todos: Vec<Todo>,
    },
    /// The provider said what the conversation weighs.
    Usage {
        /// Which thread.
        thread: ThreadId,
        /// Tokens in the context window.
        context_tokens: u64,
        /// The window those tokens sit in. Zero when unsaid.
        context_limit: u64,
    },
    /// Something a person should hear about, short of an error: a refused mode, a fallback
    /// taken.
    Noted {
        /// Which thread.
        thread: ThreadId,
        /// What happened.
        message: String,
    },
    /// The turn ended.
    TurnDone {
        /// Which thread.
        thread: ThreadId,
        /// What went wrong, when something did.
        error: Option<String>,
        /// What the turn cost, when the provider says.
        cost_usd: Option<f64>,
    },
    /// The process died out from under a turn.
    Fatal {
        /// Which thread.
        thread: ThreadId,
        /// What happened.
        error: String,
    },
}

impl AgentEvent {
    /// Which thread the event is about.
    #[must_use]
    pub fn thread(&self) -> ThreadId {
        match self {
            Self::SessionStarted { thread, .. }
            | Self::Catalog { thread, .. }
            | Self::State { thread, .. }
            | Self::Delta { thread, .. }
            | Self::Work { thread, .. }
            | Self::Runners { thread, .. }
            | Self::Asked { thread, .. }
            | Self::AskGone { thread, .. }
            | Self::PlanProposed { thread, .. }
            | Self::Todos { thread, .. }
            | Self::Usage { thread, .. }
            | Self::Noted { thread, .. }
            | Self::TurnDone { thread, .. }
            | Self::Fatal { thread, .. } => *thread,
        }
    }
}
