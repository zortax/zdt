//! The agent surface: what it shows, and how it is drawn.
//!
//! Two pieces sharing one state object. The sidebar lists every thread over every project, and
//! the chat view is the full conversation of the one selected. A toggle at the top of the
//! sidebar switches the window between the editor and the chat; both stay mounted, so switching
//! costs a repaint and nothing else.
//!
//! Everything the surface needs from the editor around it goes through [`Host`], following the
//! git panel's precedent: the surface works inside an editor and equally well over a test host
//! that is nowhere.

pub mod chat;
pub mod host;
pub mod sidebar;
pub mod state;

pub use crate::host::{Host, Nowhere, Offer};
pub use crate::state::{
    AgentUi, Committing, MenuKind, MenuRow, Review, Screen, Shelf, SideRow, Want,
};

/// What the sidebar's keys are bound in.
pub const REGION: &str = "agent";

/// What the chat view's keys are bound in.
pub const REGION_CHAT: &str = "agent-chat";

/// What the review surface's keys are bound in.
pub const REGION_DIFF: &str = "agent-diff";

/// The keys the sidebar ships with.
pub const KEYMAP: &str = include_str!("../assets/keymap-agent.toml");

/// The keys the chat view ships with.
pub const KEYMAP_CHAT: &str = include_str!("../assets/keymap-agent-chat.toml");

/// The keys the review surface ships with.
pub const KEYMAP_DIFF: &str = include_str!("../assets/keymap-agent-diff.toml");

/// How the surface is drawn.
pub const STYLE: &str = include_str!("../assets/css/agent.css");

/// Puts the surface's state where every component can find it.
pub fn provide(agent: AgentUi) {
    zgui::reactive::provide_local_context(agent);
}

/// It, from inside a component.
///
/// # Panics
///
/// If none was provided above this component, which is a wiring mistake.
#[must_use]
pub fn use_agent() -> AgentUi {
    zgui::reactive::use_local_context::<AgentUi>()
        .expect("an agent surface is provided at the root")
}

/// It, when there is one.
#[must_use]
pub fn try_use_agent() -> Option<AgentUi> {
    zgui::reactive::use_local_context::<AgentUi>()
}
