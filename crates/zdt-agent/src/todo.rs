//! The turn's own checklist.
//!
//! Providers keep a running plan of steps while they work. The daemon projects the latest one
//! per thread, and the timeline shows it as a checklist that ticks along.

use serde::{Deserialize, Serialize};

/// One step of the plan.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Todo {
    /// What the step is.
    pub text: String,
    /// Where it stands.
    pub state: TodoState,
}

/// Where one step stands.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoState {
    /// Not begun.
    #[default]
    Pending,
    /// Underway.
    Active,
    /// Finished.
    Done,
    /// A state this release has no word for.
    #[serde(other)]
    Unknown,
}
