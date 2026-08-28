//! The Excalidraw editor: the drawing on a plane, the tools over it, and the keys that work them.
//!
//! The editor is given a [`Board`] and draws it. Every change goes through a command, and a host
//! hears about one through the board's revision or through a [`view::Sink`] — the editor itself
//! never touches a file.

pub mod actions;
pub mod bar;
pub mod clipboard;
pub mod color;
pub mod fonts;
pub mod handles;
pub mod layers;
pub mod library;
pub mod overlay;
pub mod pointer;
pub mod state;
pub mod text;
pub mod view;
pub mod viewport;

pub use crate::actions::{KEYMAP, REGION, STYLE};
pub use crate::state::{Board, Tool};
pub use crate::view::{Editor, EditorProps, Sink};
pub use crate::viewport::Viewport;
