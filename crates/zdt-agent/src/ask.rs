//! What a turn stops to ask.
//!
//! A provider that wants permission, or an answer, blocks its turn until somebody decides. The
//! daemon holds the open asks; the editor shows them in the composer's place and sends one
//! decision back.

use serde::{Deserialize, Serialize};

use crate::thread::ToolKind;

/// One open question from a running turn.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Ask {
    /// The provider's name for the request. Opaque; a decision hands it back.
    pub id: String,
    /// What is being asked.
    pub kind: AskKind,
}

/// What an ask wants.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AskKind {
    /// Permission to use a tool.
    Tool {
        /// The tool's name, as the provider says it.
        name: String,
        /// What sort of tool it is.
        tool: ToolKind,
        /// One line saying what it would do.
        summary: String,
        /// The whole input, for reading before deciding.
        detail: String,
    },
    /// A question with options to choose from.
    Question {
        /// The questions, answered together.
        questions: Vec<Question>,
    },
    /// An ask this release has no word for.
    #[default]
    #[serde(other)]
    Unknown,
}

/// One question inside an ask.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Question {
    /// The question itself.
    pub question: String,
    /// A short label naming what it decides.
    pub header: String,
    /// The choices.
    pub options: Vec<QuestionOption>,
    /// Whether several choices may be taken together.
    pub multi: bool,
}

/// One choice.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct QuestionOption {
    /// What the choice is called.
    pub label: String,
    /// What taking it means.
    pub description: String,
}

/// What was decided about a tool ask.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Decision {
    /// Run it.
    Allow,
    /// Run it, and stop asking about this for the session.
    AllowAlways,
    /// Do not run it.
    Deny,
    /// A decision this release has no word for.
    #[serde(other)]
    Unknown,
}
