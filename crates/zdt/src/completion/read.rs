//! What the interface reads.

use super::*;

impl Completion {
    // ---- What the interface reads ------------------------------------------------------------

    /// Whether the popup is up, without subscribing.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.inner.open.with_untracked(Option::is_some)
    }

    /// Where it is, when it is up. Tracked.
    #[must_use]
    pub fn open(&self) -> Option<Open> {
        self.inner.open.get()
    }

    /// What it is showing. Tracked.
    #[must_use]
    pub fn items(&self) -> Vec<Item> {
        self.inner.items.get()
    }

    /// Which row the caret is on. Tracked.
    #[must_use]
    pub fn at(&self) -> usize {
        self.inner.at.get()
    }

    /// The documentation beside it, when there is any. Tracked.
    #[must_use]
    pub fn docs(&self) -> Option<Vec<crate::markdown::Block>> {
        self.inner.docs.get()
    }

    /// How far it has been scrolled. Tracked.
    #[must_use]
    pub fn docs_offset(&self) -> f32 {
        self.inner.docs_offset.get()
    }
}
