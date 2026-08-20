//! Which mode the keys are in.
//!
//! Derived from the focus, and held nowhere. The status line and the key filter read this one
//! function, so what is shown and where a key goes cannot disagree.

use zdt_vim::Mode;

use super::{Focus, Focusing, Overlay};
use crate::terminals::Terminals;
use crate::vim::Vim;
use crate::workspace::Workspace;

impl Focusing {
    /// What mode the editor is in, as the status line names it and the keymap resolves it.
    ///
    /// A terminal answers for itself: while a program is reading the keys, what the engine thinks
    /// says nothing about where they go. Only the terminal that has the keyboard may answer, which
    /// is why this asks the focus first and a terminal second.
    #[must_use]
    pub fn mode(&self, vim: &Vim, terminals: Option<&Terminals>, workspace: &Workspace) -> Mode {
        let inserting = |buffer| terminals.is_some_and(|held| held.is_inserting(buffer));

        match self.current() {
            Focus::Overlay(Overlay::CommandLine) => Mode::Command,
            Focus::Overlay(Overlay::Float(buffer)) if inserting(buffer) => Mode::Terminal,
            Focus::Overlay(_) | Focus::Tree => Mode::Normal,
            Focus::Window(window) => {
                let Some(buffer) = workspace
                    .window(window)
                    .and_then(|state| state.current)
                    .and_then(|id| workspace.buffer(id))
                else {
                    // A window with nothing in it. Nothing to type into.
                    return Mode::Normal;
                };
                if buffer.is_terminal() {
                    if inserting(buffer.id) {
                        Mode::Terminal
                    } else {
                        Mode::Normal
                    }
                } else if buffer.is_panel() {
                    // A panel is a page: no caret, and the engine is not answering for it.
                    Mode::Normal
                } else {
                    vim.mode()
                }
            }
        }
    }
}
