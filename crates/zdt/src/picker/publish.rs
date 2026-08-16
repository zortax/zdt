//! Putting rows where the interface reads them.

use super::*;

impl Picker {
    // ---- Publishing --------------------------------------------------------------------------

    /// Puts `rows` where the list reads them, keeping the caret in range.
    pub(super) fn publish(&self, rows: Vec<Row>) {
        let count = rows.len();
        self.inner.rows.set(rows);
        let at = self.inner.at.get_untracked();
        if count == 0 {
            if at != 0 {
                self.inner.at.set(0);
            }
        } else if at >= count {
            self.inner.at.set(count - 1);
        }
    }

    /// Adds `rows` to what is already shown, up to `limit`.
    ///
    /// What a live source does: the first hits are drawn while the rest are still being found.
    pub(super) fn extend(&self, rows: Vec<Row>, limit: usize) {
        if rows.is_empty() {
            return;
        }
        // The first batch of a new search replaces what the last one left; every batch after it
        // adds to what this one has found.
        let mut held = if self.inner.stale.replace(false) {
            self.inner.at.set(0);
            Vec::new()
        } else {
            self.inner.rows.get_untracked()
        };
        if held.len() >= limit {
            return;
        }
        let room = limit - held.len();
        held.extend(rows.into_iter().take(room));
        let count = held.len();
        self.inner.rows.set(held);
        self.inner.counts.set((count, count));
    }
}
