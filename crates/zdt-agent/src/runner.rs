//! Work running beside the main agent: subagents, workflows, background commands.
//!
//! A provider can keep working after its turn ends — an agent launched into the background, a
//! workflow fanning out over many agents. Each is a runner. The daemon holds the live set per
//! thread and pushes it whole on every change, so a client always paints the current picture
//! and never merges.

use serde::{Deserialize, Serialize};

/// What sort of thing a runner is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunnerKind {
    /// A subagent working apart.
    #[default]
    Agent,
    /// A workflow orchestrating agents of its own.
    Workflow,
    /// A background command.
    Shell,
    /// Something this build has no word for.
    #[serde(other)]
    Other,
}

/// One piece of work running beside the main agent.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Runner {
    /// The provider's id for it.
    pub id: String,
    /// What sort of thing it is.
    pub kind: RunnerKind,
    /// What it was asked to do, in a line.
    pub description: String,
    /// The agent type driving it, when it is an agent. Empty otherwise.
    pub agent_type: String,
    /// Whether it runs in the background, past the turn that started it.
    pub background: bool,
    /// Tokens it has spent so far. Zero when unsaid.
    pub tokens: u64,
    /// Tool calls it has made so far.
    pub tool_uses: u32,
    /// How long it has run, in milliseconds. Zero when unsaid.
    pub duration_ms: u64,
    /// The last tool it reached for. Empty when unsaid.
    pub last_tool: String,
    /// The provider's one-line summary of where it stands. Empty when unsaid.
    pub summary: String,
    /// The workflow behind it, when it is one.
    pub workflow: Option<WorkflowRun>,
}

/// A workflow's live picture: its phases, the agents inside them, and what it logged.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkflowRun {
    /// The workflow's own name, from its metadata.
    pub name: String,
    /// The phase titles, in order.
    pub phases: Vec<String>,
    /// Every agent the workflow has spawned, in the order they appeared.
    pub agents: Vec<WorkflowAgent>,
    /// What the script logged, in order.
    pub logs: Vec<String>,
}

/// One agent inside a workflow.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkflowAgent {
    /// The workflow's label for it.
    pub label: String,
    /// The phase it belongs to, by title.
    pub phase: String,
    /// The model it runs on. Empty when unsaid.
    pub model: String,
    /// Where it stands: `start`, `progress`, `done`, or `error`.
    pub state: String,
    /// Tokens it has spent.
    pub tokens: u64,
    /// Tool calls it has made.
    pub tool_calls: u32,
    /// How long it has run, in milliseconds.
    pub duration_ms: u64,
    /// The last tool it reached for.
    pub last_tool: String,
    /// The provider's one-line summary of its last step.
    pub last_summary: String,
}

impl WorkflowAgent {
    /// Whether the agent is still going.
    #[must_use]
    pub fn is_live(&self) -> bool {
        matches!(self.state.as_str(), "start" | "progress")
    }
}
