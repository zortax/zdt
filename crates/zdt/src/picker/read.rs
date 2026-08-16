//! What the interface reads.

use super::*;

impl Picker {
    // ---- What the interface reads ------------------------------------------------------------

    /// Which picker is open. Tracked.
    #[must_use]
    pub fn source(&self) -> Option<Source> {
        self.inner.source.get()
    }

    /// Whether one is. Tracked.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.inner.source.with(Option::is_some)
    }

    /// What has been typed. Tracked.
    #[must_use]
    pub fn query(&self) -> String {
        self.inner.query.get()
    }

    /// The rows. Tracked.
    #[must_use]
    pub fn rows(&self) -> Vec<Row> {
        self.inner.rows.get()
    }

    /// How many rows there are. Tracked, and narrower than reading them.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.rows.with(Vec::len)
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Which row the caret is on. Tracked.
    #[must_use]
    pub fn at(&self) -> usize {
        self.inner.at.get()
    }

    /// How many matched, and how many there were. Tracked.
    #[must_use]
    pub fn counts(&self) -> (usize, usize) {
        self.inner.counts.get()
    }

    /// Whether anything is still being gathered. Tracked.
    #[must_use]
    pub fn is_working(&self) -> bool {
        self.inner.working.get()
    }

    /// The row the caret is on.
    #[must_use]
    pub fn selected(&self) -> Option<Row> {
        self.inner
            .rows
            .with_untracked(|rows| rows.get(self.inner.at.get_untracked()).cloned())
    }
}
