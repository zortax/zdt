//! Documents, project state, configuration and search.
//!
//! Everything in this crate is plain data and plain threads. Nothing here names the user
//! interface, so a file can be read, searched and written in a test with no window open.

pub mod config;
pub mod fs;
pub mod language;
pub mod paths;
pub mod project;
pub mod search;
pub mod state;
pub mod theme;
pub mod tree;
pub mod watch;

pub use crate::config::{Config, Paths};
pub use crate::fs::{Encoding, FileError, LineEnding, LoadedFile};
pub use crate::language::FileType;
pub use crate::project::Project;
pub use crate::theme::{ThemeSource, builtin_theme, builtin_theme_names};
pub use crate::tree::Tree;
