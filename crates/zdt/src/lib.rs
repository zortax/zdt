//! zdt — a fast, vim-native code editor.
//!
//! The application is a library, so a test with no window open can drive everything in it: the
//! workspace, the vim layer, the pickers. The binary is the entry point and nothing else.
//!
//! # How the modules are laid out
//!
//! One directory per region of the editor, each holding both what that region knows and what
//! draws it. `picker` is the picker's state, its sources and its modal; `explorer` is the file
//! tree, its rows and its context menu. Nothing collects components because they are components.
//!
//! What several regions share is a crate: [`zdt_view`] for the pieces every view needs,
//! [`zdt_icons`] for the icon set, and [`zdt_gitui`] for the git panel.

pub mod actions;
pub mod app;
pub mod assets;
pub mod cli;
pub mod cmdline;
pub mod completion;
pub mod explorer;
pub mod files;
pub mod focus;
pub mod git;
pub mod hover;
pub mod keymaps;
pub mod keys;
pub mod language;
pub mod leap;
pub mod markdown;
pub mod notify;
pub mod picker;
pub mod prompt;
pub mod reload;
pub mod rename;
pub mod session;
pub mod settings;
pub mod tabpick;
pub mod terminals;
pub mod vim;
pub mod workspace;
