//! What a terminal is doing with the keys.
//!
//! Two answers, and the second one has a mode of its own inside it. While the program is reading,
//! nothing else may: the keymap is asked in [`Mode::Terminal`](zdt_vim::Mode::Terminal), where
//! almost nothing is bound, and everything else goes to the program. While the keymap is reading,
//! the vim engine drives what the terminal holds, and *it* names the mode — NORMAL, VISUAL, and
//! the rest — exactly as it does over a file.
//!
//! Held per terminal, the way it is in vim: walking out of a split and back finds the terminal as
//! it was left, and a terminal nobody is looking at names no mode at all. Which of them has the
//! keyboard is [`crate::focus`]'s question.

use zgui::reactive::prelude::*;

use super::Terminals;
use crate::vim::Vim;
use crate::workspace::BufferId;

/// What a terminal is doing with the keys.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum TerminalMode {
    /// The program reads them. Vim's terminal mode.
    #[default]
    Terminal,
    /// The keymap reads them, over what the terminal holds. Vim's terminal-normal mode.
    Normal,
}

impl Terminals {
    /// What `buffer` is doing with the keys. Tracked.
    #[must_use]
    pub fn mode_of(&self, buffer: BufferId) -> TerminalMode {
        self.inner
            .modes
            .with(|held| held.get(&buffer).copied().unwrap_or_default())
    }

    /// The same, without subscribing.
    #[must_use]
    pub fn mode_of_untracked(&self, buffer: BufferId) -> TerminalMode {
        self.inner
            .modes
            .with_untracked(|held| held.get(&buffer).copied().unwrap_or_default())
    }

    /// Gives the keys to the program in `buffer`, at its own cursor.
    ///
    /// This is vim's terminal mode. The engine is put back to normal on the way in, because what
    /// it was part-way through belongs to whatever was being read before.
    pub fn enter_terminal_mode(&self, vim: &Vim, buffer: BufferId) {
        vim.reset();
        if let Some(scrollback) = self.inner.normals.borrow_mut().remove(&buffer) {
            scrollback.hide_cursor();
        }
        if let Some(handle) = self.handle(buffer) {
            // Typing goes where the program left its cursor, so the screen goes back there too.
            handle.scroll(zgui_terminal::ScrollRequest::Bottom);
        }
        self.set_mode(buffer, TerminalMode::Terminal);
    }

    /// Takes the keys back, leaving the caret where the program's cursor is.
    ///
    /// This is vim's `<C-\><C-n>`. The terminal stays where it is and the program keeps running;
    /// what changes is that the keymap answers again, so what the terminal holds can be walked
    /// with vim's own motions.
    pub fn enter_normal_mode(&self, vim: &Vim, buffer: BufferId) {
        vim.reset();
        self.set_mode(buffer, TerminalMode::Normal);
        // The caret belongs to the view that draws the grid. A terminal nobody is drawing has
        // none yet, and gets one when a view registers.
        if let Some(handle) = self.handle(buffer) {
            let scrollback = super::normal::Scrollback::new(buffer, handle);
            scrollback.show_cursor();
            self.inner.normals.borrow_mut().insert(buffer, scrollback);
        }
    }

    /// The contents of `buffer`, when its keys are being read rather than typed.
    #[must_use]
    pub fn normal(&self, buffer: BufferId) -> Option<super::normal::Scrollback> {
        self.inner.normals.borrow().get(&buffer).cloned()
    }

    /// Takes the visual painting off every terminal that has a caret.
    ///
    /// What [`Vim::reset`] asks for: nothing is selected in a mode nobody is in any more. The
    /// carets stay where they are, so coming back to a terminal still finds it where it was left.
    pub(crate) fn clear_paint(&self) {
        for scrollback in self.inner.normals.borrow().values() {
            scrollback.unpaint();
        }
    }

    /// Forgets everything a terminal was doing with the keys, which closing one does.
    pub(super) fn forget_mode(&self, buffer: BufferId) {
        self.inner.normals.borrow_mut().remove(&buffer);
        self.inner.waiting.borrow_mut().remove(&buffer);
        if self
            .inner
            .modes
            .with_untracked(|held| held.contains_key(&buffer))
        {
            self.inner.modes.update(|held| {
                held.remove(&buffer);
            });
        }
    }

    fn set_mode(&self, buffer: BufferId, mode: TerminalMode) {
        if self.mode_of_untracked(buffer) != mode {
            self.inner.modes.update(|held| {
                held.insert(buffer, mode);
            });
        }
    }
}
