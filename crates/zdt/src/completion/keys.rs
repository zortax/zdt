//! What the keys do.

use super::*;

impl Completion {
    // ---- What the keys do --------------------------------------------------------------------

    /// Moves the caret by `offset` rows, wrapping.
    ///
    /// Wrapping because a list of suggestions has no ends worth stopping at: `<C-p>` from the top
    /// meaning "the last one" is what every completion anybody has used does.
    pub fn step(&self, offset: isize) {
        let count = self.inner.items.with_untracked(Vec::len);
        if count == 0 {
            return;
        }
        let at = self.inner.at.get_untracked() as isize;
        let next = (at + offset).rem_euclid(count as isize) as usize;
        if next != self.inner.at.get_untracked() {
            self.inner.at.set(next);
            self.forget_docs();
            self.want_docs();
        }
    }

    /// Puts the popup away.
    pub fn close(&self) {
        // The generation moves, so an answer already on its way is dropped. It would otherwise
        // reopen the popup somebody just dismissed.
        self.inner.generation.set(self.inner.generation.get() + 1);
        self.inner.pending.borrow_mut().take();
        self.forget_docs();
        if self.is_open() {
            self.inner.open.set(None);
        }
        if !self.inner.items.with_untracked(Vec::is_empty) {
            self.inner.items.set(Vec::new());
        }
        self.inner.all.borrow_mut().clear();
        self.inner.at.set(0);
    }

    /// Moves the documentation panel by `lines`.
    pub fn scroll_docs(&self, lines: f32) {
        let next = (self.inner.docs_offset.get_untracked() + lines * 16.0).max(0.0);
        self.inner.docs_offset.set(next);
    }
}
