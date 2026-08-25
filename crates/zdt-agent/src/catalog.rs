//! What a live session offers: its commands, its skills, and its models.

use serde::{Deserialize, Serialize};

/// One slash command a session answers to.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SlashCommand {
    /// The name, without the leading slash.
    pub name: String,
    /// What it does, in the provider's words.
    pub description: String,
}

/// One model the provider offers.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelChoice {
    /// The word the provider takes, e.g. `sonnet`. `default` means its own choice.
    pub id: String,
    /// What the picker shows.
    pub label: String,
    /// One line saying what it is for.
    pub description: String,
}

/// One reasoning-effort level the provider offers.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EffortChoice {
    /// The word the provider takes, e.g. `high`. `default` means its own choice.
    pub id: String,
    /// What the picker shows.
    pub label: String,
    /// One line saying what it is for.
    pub description: String,
}

/// Everything a session says it offers.
///
/// Fields arrive from different messages at different times; an empty field means "not said
/// yet", and merging keeps whatever was known.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Catalog {
    /// The slash commands.
    pub commands: Vec<SlashCommand>,
    /// The skill names.
    pub skills: Vec<String>,
    /// The models.
    pub models: Vec<ModelChoice>,
    /// The reasoning-effort levels.
    pub efforts: Vec<EffortChoice>,
}

impl Catalog {
    /// Takes every field `newer` actually says, and keeps the rest.
    pub fn merge(&mut self, newer: Catalog) {
        if !newer.commands.is_empty() {
            self.commands = newer.commands;
        }
        if !newer.skills.is_empty() {
            self.skills = newer.skills;
        }
        if !newer.models.is_empty() {
            self.models = newer.models;
        }
        if !newer.efforts.is_empty() {
            self.efforts = newer.efforts;
        }
    }

    /// Whether nothing has been said yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
            && self.skills.is_empty()
            && self.models.is_empty()
            && self.efforts.is_empty()
    }
}
