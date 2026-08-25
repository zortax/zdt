//! How much a thread's agent may do unasked.

use serde::{Deserialize, Serialize};

/// A thread's permission level.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeMode {
    /// Every consequential tool asks first.
    #[default]
    Supervised,
    /// File edits run unasked; everything else asks.
    AcceptEdits,
    /// The provider decides by its own rules.
    Auto,
    /// Nothing asks.
    Full,
    /// Read-only planning; the plan itself comes back for approval.
    Plan,
    /// A mode this release has no word for.
    #[serde(other)]
    Unknown,
}

impl RuntimeMode {
    /// The word the database stores.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Supervised => "supervised",
            Self::AcceptEdits => "accept_edits",
            Self::Auto => "auto",
            Self::Full => "full",
            Self::Plan => "plan",
            Self::Unknown => "unknown",
        }
    }

    /// The mode a stored word names.
    #[must_use]
    pub fn named(word: &str) -> Self {
        match word {
            "supervised" => Self::Supervised,
            "accept_edits" => Self::AcceptEdits,
            "auto" => Self::Auto,
            "full" => Self::Full,
            "plan" => Self::Plan,
            _ => Self::Unknown,
        }
    }

    /// What the interface calls it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Supervised => "Supervised",
            Self::AcceptEdits => "Accept edits",
            Self::Auto => "Auto",
            Self::Full => "Full access",
            Self::Plan => "Plan",
            Self::Unknown => "Unknown",
        }
    }

    /// One line saying what the mode means.
    #[must_use]
    pub fn blurb(self) -> &'static str {
        match self {
            Self::Supervised => "Every consequential tool asks first",
            Self::AcceptEdits => "File edits run unasked; commands still ask",
            Self::Auto => "The provider decides by its own rules",
            Self::Full => "Nothing asks",
            Self::Plan => "Read-only planning; the plan comes back for approval",
            Self::Unknown => "",
        }
    }

    /// Every mode a person can choose, in the order the picker shows.
    pub const CHOICES: [Self; 5] = [
        Self::Supervised,
        Self::AcceptEdits,
        Self::Auto,
        Self::Full,
        Self::Plan,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mode_survives_the_round_trip_through_its_word() {
        for mode in RuntimeMode::CHOICES {
            assert_eq!(RuntimeMode::named(mode.word()), mode);
        }
    }
}
