//! zdt — a fast, vim-native code editor.
//!
//! The application is a library so that everything in it — the workspace, the vim layer, the
//! pickers — can be driven by a test with no window open. The binary is the entry point and
//! nothing else.

pub mod app;
pub mod assets;
pub mod files;
// The icon set is chosen as a set: an outline drawn only by a region built later is still part of
// it, and the props of `Icon` are its contract whether or not a caller has needed one yet.
#[allow(dead_code)]
pub mod icons;
pub mod keys;
pub mod ui;
pub mod workspace;
