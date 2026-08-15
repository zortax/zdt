//! The modal editing engine and the keymap.
//!
//! Nothing here names a user-interface crate. A chord goes in, a list of actions comes out, and
//! every motion, operator and text object is a function from a borrowed rope and a cursor to a
//! byte range — which is what makes the whole grammar testable by asserting on text.

pub mod action;
pub mod chord;
pub mod config;
pub mod effect;
pub mod engine;
pub mod keymap;
pub mod leap;
pub mod mode;
pub mod motion;
pub mod notation;
pub mod register;
pub mod text;
pub mod textobject;

pub use crate::action::{Action, Args};
pub use crate::chord::{Chord, Key, Mods, Named};
pub use crate::effect::{Context, Effect, Scroll, Selection, Step};
pub use crate::engine::Engine;
pub use crate::keymap::{Binding, Continuation, Keymap, Layered, Resolution};
pub use crate::mode::{Mode, ModeSet};
pub use crate::notation::Leaders;
