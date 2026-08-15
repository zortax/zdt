//! zdt — a fast, vim-native code editor.
//!
//! The application is a library so that everything in it — the workspace, the vim layer, the
//! pickers — can be driven by a test with no window open. The binary is the entry point and
//! nothing else.

pub mod actions;
pub mod app;
pub mod assets;
pub mod cmdline;
pub mod explorer;
pub mod files;
pub mod git;
// The icon set is chosen as a set: an outline drawn only by a region built later is still part of
// it, and the props of `Icon` are its contract whether or not a caller has needed one yet.
#[allow(dead_code)]
pub mod icons;
pub mod keys;
pub mod language;
pub mod leap;
pub mod picker;
pub mod prompt;
pub mod reload;
pub mod session;
pub mod settings;
pub mod tabpick;
pub mod task;
pub mod terminals;
pub mod ui;
pub mod vim;
pub mod workspace;
