//! The terminals, drawn.
//!
//! One component for both kinds. A terminal in a window and a terminal floating over one differ
//! in where they are placed and nothing else. Same emulator, same keys, same way out.
//!
//! # Which layer gets the key
//!
//! While a terminal is being typed into, almost every key belongs to the program: `j` is a `j`,
//! `<Esc>` is an escape, and a keymap that answered either would make a terminal nobody can use.
//! Three things are kept back:
//!
//! * `<C-\><C-n>`, which is vim's way out of terminal mode. After it the keymap answers again and
//!   the scrollback can be walked with ordinary motions.
//! * `<C-h/j/k/l>`, which move between windows. Vim's own terminal mode maps these for the same
//!   reason: leaving a terminal must not need two hands.
//! * the key that toggles the float, so that the thing that opened it can put it away.

mod emulator;
mod float;

pub use crate::terminals::view::emulator::{Emulator, EmulatorProps};
pub use crate::terminals::view::float::{FloatingTerminal, FloatingTerminalProps};
