//! The terminals, drawn.
//!
//! One component for both kinds. A terminal in a window and a terminal floating over one differ
//! in where they are placed and nothing else. Same emulator, same keys, same way out.
//!
//! # Which layer gets the key
//!
//! While a program is reading, every key belongs to it unless the keymap binds that key in
//! terminal mode: `j` is a `j`, `<Esc>` is an escape, `<C-l>` clears the screen. What is bound
//! there is `assets/keymap-terminal.toml`, and it is two rows — the way out and the key that puts
//! a float away. Everything else a person wants kept back is a row they add.
//!
//! After `<C-\><C-n>` the keymap answers again and the vim engine drives what the terminal holds,
//! so motions, text objects and the visual modes work over it exactly as they do over a file. See
//! [`mode`](crate::terminals::mode) for the two states and
//! [`normal`](crate::terminals::normal) for the surface the engine reads.

pub(crate) mod emulator;
mod float;

pub use crate::terminals::view::emulator::{Emulator, EmulatorProps};
pub use crate::terminals::view::float::{FloatingTerminal, FloatingTerminalProps};
