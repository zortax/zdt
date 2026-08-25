//! Threads, and what a client shows of one.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Names one thread for as long as the daemon's database lives.
#[derive(
    Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct ThreadId(pub i64);

impl std::fmt::Display for ThreadId {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(out, "{}", self.0)
    }
}

/// Where a thread stands, as the sidebar shows it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadState {
    /// Nothing running, nothing owed.
    #[default]
    Idle,
    /// A turn was asked for and the provider is coming up.
    Starting,
    /// A turn is running.
    Working,
    /// The last turn ended badly. The message is on the shell.
    Failed,
    /// A state this release has no word for.
    #[serde(other)]
    Unknown,
}

impl ThreadState {
    /// The word the database stores.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Starting => "starting",
            Self::Working => "working",
            Self::Failed => "failed",
            Self::Unknown => "unknown",
        }
    }

    /// The state a stored word names.
    #[must_use]
    pub fn named(word: &str) -> Self {
        match word {
            "idle" => Self::Idle,
            "starting" => Self::Starting,
            "working" => Self::Working,
            "failed" => Self::Failed,
            _ => Self::Unknown,
        }
    }

    /// Whether a turn is underway.
    #[must_use]
    pub fn is_busy(self) -> bool {
        matches!(self, Self::Starting | Self::Working)
    }
}

/// One thread, as the sidebar lists it.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ThreadShell {
    /// Which thread.
    pub id: ThreadId,
    /// The directory it works in: the worktree for a worktree thread, the project otherwise.
    pub root: PathBuf,
    /// The project's own directory, where the repository's main checkout is.
    pub project_root: PathBuf,
    /// What the project is called: the directory's last component.
    pub project: String,
    /// Whether the thread works in a worktree of its own.
    pub worktree: bool,
    /// The branch the thread works on. Empty for a thread in the main checkout.
    pub branch: String,
    /// The branch its directory actually has checked out, when [`Self::branch`] is set.
    pub on_branch: String,
    /// What the thread's turns have changed so far, checkpoint to checkpoint.
    pub changed: DiffStat,
    /// Which configured provider instance drives it.
    pub instance: String,
    /// Which harness that instance is: `claude` or `codex`. Empty on rows from older daemons.
    pub provider: String,
    /// What the thread is called.
    pub title: String,
    /// Its place among the pinned threads, highest first. Zero means not pinned.
    pub pinned: f64,
    /// When its snooze ends, in milliseconds since the epoch. Zero means not snoozed. A moment
    /// already past means it woke and nobody has looked yet.
    pub snoozed_until: u64,
    /// Whether it is put away as done.
    pub settled: bool,
    /// Whether it is archived.
    pub archived: bool,
    /// Whether it finished something nobody has read.
    pub unread: bool,
    /// The prompt typed into its composer and not sent yet.
    pub draft: String,
    /// Where it stands.
    pub state: ThreadState,
    /// What went wrong, when the state says failed.
    pub last_error: Option<String>,
    /// How much its agent may do unasked.
    pub mode: crate::mode::RuntimeMode,
    /// Which model it talks to. Empty means the provider's default.
    pub model: String,
    /// How hard it reasons, in the provider's own words. Empty means the provider's default.
    pub effort: String,
    /// How many asks wait on a person.
    pub asking: u32,
    /// How many runners — subagents, workflows — work beside the main agent right now.
    pub runners: u32,
    /// Whether a proposed plan waits for approval.
    pub planned: bool,
    /// What the conversation weighs by now.
    pub usage: Usage,
    /// When it was made, in milliseconds since the epoch.
    pub created_at_ms: u64,
    /// When it last moved.
    pub updated_at_ms: u64,
}

impl ThreadShell {
    /// Whether the thread has anything going: a turn, or runners working beside one.
    #[must_use]
    pub fn is_working(&self) -> bool {
        self.state.is_busy() || self.runners > 0
    }
}

/// How much a thread has changed, in counts.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DiffStat {
    /// Files touched.
    pub files: u32,
    /// Lines added.
    pub added: u32,
    /// Lines taken away.
    pub removed: u32,
}

impl DiffStat {
    /// Whether anything changed at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files == 0
    }
}

/// What a thread's conversation weighs.
#[derive(Clone, Copy, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Usage {
    /// Tokens in the context window after the last turn.
    pub context_tokens: u64,
    /// The window those tokens sit in. Zero when the provider has not said.
    pub context_limit: u64,
    /// What every turn so far cost, in dollars.
    pub cost_usd: f64,
}

/// What one timeline row is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// Something the person said.
    User,
    /// Something the agent said.
    #[default]
    Assistant,
    /// The agent thinking out loud.
    Thinking,
    /// One tool run.
    Tool,
    /// A subagent working apart.
    Task,
    /// What a settled turn changed, checkpoint to checkpoint.
    Diff,
    /// A kind this release has no word for.
    #[serde(other)]
    Unknown,
}

impl ItemKind {
    /// The word the database stores.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Thinking => "thinking",
            Self::Tool => "tool",
            Self::Task => "task",
            Self::Diff => "diff",
            Self::Unknown => "unknown",
        }
    }

    /// The kind a stored word names.
    #[must_use]
    pub fn named(word: &str) -> Self {
        match word {
            "user" => Self::User,
            "assistant" => Self::Assistant,
            "thinking" => Self::Thinking,
            "tool" => Self::Tool,
            "task" => Self::Task,
            "diff" => Self::Diff,
            _ => Self::Unknown,
        }
    }
}

/// What sort of work a tool row is, for its glyph.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    /// Reading files or listing what is there.
    Read,
    /// Changing files.
    Edit,
    /// Running commands.
    Execute,
    /// Searching the tree.
    Search,
    /// Fetching or searching the web.
    Web,
    /// Keeping the plan checklist.
    Plan,
    /// A tool from an MCP server.
    Mcp,
    /// A kind this release has no word for.
    #[default]
    #[serde(other)]
    Other,
}

impl ToolKind {
    /// The word the database stores.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Edit => "edit",
            Self::Execute => "execute",
            Self::Search => "search",
            Self::Web => "web",
            Self::Plan => "plan",
            Self::Mcp => "mcp",
            Self::Other => "other",
        }
    }

    /// The kind a stored word names.
    #[must_use]
    pub fn named(word: &str) -> Self {
        match word {
            "read" => Self::Read,
            "edit" => Self::Edit,
            "execute" => Self::Execute,
            "search" => Self::Search,
            "web" => Self::Web,
            "plan" => Self::Plan,
            "mcp" => Self::Mcp,
            _ => Self::Other,
        }
    }
}

/// Where a tool or task row stands.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    /// Still going.
    Running,
    /// Finished well.
    #[default]
    Ok,
    /// Finished badly.
    Failed,
    /// Refused before it ran.
    Declined,
    /// A status this release has no word for.
    #[serde(other)]
    Unknown,
}

impl ItemStatus {
    /// The word the database stores.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Ok => "ok",
            Self::Failed => "failed",
            Self::Declined => "declined",
            Self::Unknown => "unknown",
        }
    }

    /// The status a stored word names.
    #[must_use]
    pub fn named(word: &str) -> Self {
        match word {
            "running" => Self::Running,
            "ok" => Self::Ok,
            "failed" => Self::Failed,
            "declined" => Self::Declined,
            _ => Self::Unknown,
        }
    }
}

/// One row of a thread's conversation.
///
/// Rows written down carry their database id. The one assistant message still streaming carries
/// [`LIVE_ASSISTANT`], and the thinking beside it [`LIVE_THINKING`], so a delta knows which row
/// it belongs to before anything is persisted.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TimelineItem {
    /// Which row, within its thread.
    pub id: i64,
    /// What it is.
    pub kind: ItemKind,
    /// What it says. A tool row says in one line what the tool did.
    pub text: String,
    /// The tool's name, or a task's description. Empty on prose rows.
    pub name: String,
    /// What sort of tool it is. Meaningful only on tool and task rows.
    pub tool: ToolKind,
    /// Where a tool or task stands.
    pub status: ItemStatus,
    /// What came back: output worth reading when the row is opened. Empty on prose rows.
    pub detail: String,
    /// Whether it is finished. A row still streaming grows through appends.
    pub done: bool,
    /// When it was said, in milliseconds since the epoch. Zero while streaming.
    pub at_ms: u64,
    /// How long it took, in milliseconds. Zero when nobody measured.
    pub elapsed_ms: u64,
}

/// The id of the assistant message still streaming.
pub const LIVE_ASSISTANT: i64 = -1;

/// The id of the thinking that runs beside it.
pub const LIVE_THINKING: i64 = -2;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_survives_the_round_trip_through_its_word() {
        for state in [
            ThreadState::Idle,
            ThreadState::Starting,
            ThreadState::Working,
            ThreadState::Failed,
        ] {
            assert_eq!(ThreadState::named(state.word()), state);
        }
    }

    #[test]
    fn a_state_this_release_has_no_word_for_reads_as_unknown() {
        let read: ThreadState = serde_json::from_str("\"snoozed\"").expect("it decodes");
        assert_eq!(read, ThreadState::Unknown);
    }

    #[test]
    fn a_shell_with_fields_from_a_later_release_still_reads() {
        let text = r#"{"id":3,"root":"/x","title":"t","state":"idle","pinned_at":12}"#;
        let shell: ThreadShell = serde_json::from_str(text).expect("it decodes");
        assert_eq!(shell.id, ThreadId(3));
        assert_eq!(shell.state, ThreadState::Idle);
    }
}
