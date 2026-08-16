//! The git panel: what it is looking at, and how it is drawn.
//!
//! A state object over an `Rc` of signals, the components that read it, the keys it answers, and
//! the style sheet that draws it. Everything it needs from the application around it goes through
//! [`Host`], so the panel works inside an editor and equally well inside a window that holds
//! nothing else.
//!
//! # What it is looking at
//!
//! One of two things. The panel is two panels sharing a frame:
//!
//!   * [`status`]: what has changed, split into staged and unstaged, with the diff of whichever
//!     file is selected. This is the daily-driver view.
//!   * [`history`]: the commit graph, with the details and diff of whichever commit is selected.
//!
//! They share the layout because they are the same shape: a list on the left, a diff on the
//! right. One key switches between them, and that is the whole navigation.
//!
//! # How the modules are laid out
//!
//! One directory per region of the panel, each holding both what that region knows and what draws
//! it. [`panel`] is the state they all share, and the frame they all sit in.

pub mod actions;
pub mod branches;
pub mod commit;
pub mod diff;
pub mod history;
pub mod host;
pub mod labels;
pub mod panel;
pub mod status;
mod visible;

pub use crate::diff::{DiffRow, diff_rows};
pub use crate::host::{Host, Nowhere};
pub use crate::labels::{ago, ago_short, state_mark};
pub use crate::panel::{GitModal, GitModalProps, GitPanel, GitPanelProps};
pub use crate::panel::{GitUi, List, Selected, View};

/// What the panel's keys are bound in.
pub const REGION: &str = "git";

/// The keys the panel ships with.
pub const KEYMAP: &str = include_str!("../assets/keymap-git.toml");

/// How the panel is drawn.
///
/// Belongs after the application's own sheets and before whatever decides what moves.
pub const STYLE: &str = include_str!("../assets/css/git.css");

/// Puts the panel where every component can find it.
pub fn provide(git: GitUi) {
    zgui::reactive::provide_local_context(git);
}

/// It, from inside a component.
///
/// # Panics
///
/// If none was provided above this component, which is a wiring mistake.
#[must_use]
pub fn use_gitui() -> GitUi {
    zgui::reactive::use_local_context::<GitUi>().expect("a git panel is provided at the root")
}
