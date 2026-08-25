//! What a terminal does with the keys while the program has them.
//!
//! Every key is the program's unless the keymap binds it in [`Mode::Terminal`]. That is what vim
//! does, and it is what makes `<Esc>`, `<C-u>` and `<C-l>` work in a shell: a binding written
//! without a mode letter does not reach here, so only what says `t` is ever kept back.
//!
//! # Why a key is sometimes held
//!
//! A binding can be more than one key long, and `<C-\>` alone says nothing. So a key part-way
//! through a bound sequence is held until the sequence resolves. `<C-\><C-n>` leaves terminal
//! mode; `<C-\>x` was two keys the program should have had, and it is given both, encoded as this
//! terminal encodes what is typed. Nothing is lost by waiting.
//!
//! One list holds both forms of each key, so there is nothing to keep in step.

use zdt_vim::{Chord, Mode};
use zgui::vocab::{KeyEvent, Modifiers};
use zgui_terminal::GridPoint;

use super::Terminals;
use crate::vim::{Answer, Surface, Vim};
use crate::workspace::BufferId;

/// One key a terminal has held back to see what follows it.
pub(super) struct Held {
    /// What it is, as the keymap writes it.
    chord: Chord,
    /// The same key, as the terminal would encode it.
    event: KeyEvent,
    /// What was held down with it.
    modifiers: Modifiers,
}

impl Terminals {
    /// One key for the program in `buffer`.
    ///
    /// Answers whether the terminal should be left out of it: `true` means the key was the
    /// keymap's, and `false` means the program is to have it.
    pub fn terminal_key(
        &self,
        vim: &Vim,
        buffer: BufferId,
        chord: Chord,
        event: &KeyEvent,
        modifiers: Modifiers,
    ) -> bool {
        let mut waiting = self.inner.waiting.borrow_mut();
        let held = waiting.entry(buffer).or_default();
        held.push(Held {
            chord,
            event: event.clone(),
            modifiers,
        });
        let chords: Vec<Chord> = held.iter().map(|one| one.chord).collect();
        drop(waiting);

        match vim.resolve(Some(crate::vim::surface::TERMINAL), Mode::Terminal, &chords) {
            Answer::Pending => true,
            Answer::Run(actions) => {
                self.take_waiting(buffer);
                for action in &actions {
                    crate::actions::run(&self.inner.workspace, vim, action, None);
                }
                true
            }
            // Nothing is bound, so every key that was waiting belongs to the program. The last
            // one is left to the terminal, which encodes it as it encodes anything else.
            Answer::None => {
                let mut held = self.take_waiting(buffer);
                held.pop();
                if let Some(handle) = self.handle(buffer) {
                    for one in &held {
                        handle.send_key(&one.event, one.modifiers);
                    }
                }
                false
            }
        }
    }

    /// One key for the keymap, over what the terminal in `buffer` holds.
    ///
    /// The engine drives it, exactly as it drives an editor: the same motions, the same text
    /// objects, the same visual modes. Always answers `true`, because a terminal reading its own
    /// keys reads all of them.
    pub fn normal_key(&self, vim: &Vim, buffer: BufferId, chord: Chord) -> bool {
        let Some(scrollback) = self.normal(buffer) else {
            return false;
        };
        vim.key(chord, Surface::Terminal(&scrollback));
        true
    }

    /// A pointer landing on `at` in the terminal in `buffer`.
    ///
    /// `extending` says the button is still down, which is a drag: the first cell is where the
    /// caret goes, and every cell after it is the far end of a visual selection. Both are motions
    /// the engine is told about, so what a gesture paints and what `v` paints are one thing.
    pub fn point_at(&self, vim: &Vim, buffer: BufferId, at: GridPoint, extending: bool) {
        let Some(scrollback) = self.normal(buffer) else {
            return;
        };
        vim.jump_to(at, extending, &scrollback);
    }

    /// Takes back what a terminal was holding, leaving it holding nothing.
    fn take_waiting(&self, buffer: BufferId) -> Vec<Held> {
        self.inner
            .waiting
            .borrow_mut()
            .remove(&buffer)
            .unwrap_or_default()
    }
}
