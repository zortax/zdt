//! What the interface reads.

use super::*;

impl Focusing {
    /// Where the keyboard is. Tracked.
    #[must_use]
    pub fn current(&self) -> Focus {
        match self.inner.stack.with(|stack| stack.last().copied()) {
            Some(overlay) => Focus::Overlay(overlay),
            None => match self.inner.region.get() {
                Region::Tree => Focus::Tree,
                Region::Agent => Focus::Agent,
                Region::Panes => Focus::Window(self.inner.window.get()),
            },
        }
    }

    /// The same, without subscribing.
    #[must_use]
    pub fn current_untracked(&self) -> Focus {
        match self
            .inner
            .stack
            .with_untracked(|stack| stack.last().copied())
        {
            Some(overlay) => Focus::Overlay(overlay),
            None => match self.inner.region.get_untracked() {
                Region::Tree => Focus::Tree,
                Region::Agent => Focus::Agent,
                Region::Panes => Focus::Window(self.inner.window.get_untracked()),
            },
        }
    }

    /// Whether the agent surface has the keyboard, with nothing over it. Tracked.
    #[must_use]
    pub fn in_agent(&self) -> bool {
        self.current() == Focus::Agent
    }

    /// Whether the tree has the keyboard, with nothing over it. Tracked.
    #[must_use]
    pub fn in_tree(&self) -> bool {
        self.current() == Focus::Tree
    }

    /// The same, without subscribing.
    #[must_use]
    pub fn in_tree_untracked(&self) -> bool {
        self.current_untracked() == Focus::Tree
    }

    /// Whether `overlay` has the keys. Tracked.
    ///
    /// True for the innermost one alone: a picker opened over a prompt takes every key, and the
    /// prompt underneath is waiting rather than listening.
    #[must_use]
    pub fn in_overlay(&self, overlay: Overlay) -> bool {
        self.current() == Focus::Overlay(overlay)
    }

    /// The same, without subscribing.
    #[must_use]
    pub fn in_overlay_untracked(&self, overlay: Overlay) -> bool {
        self.current_untracked() == Focus::Overlay(overlay)
    }

    /// The current pane, whatever is over it. Tracked.
    #[must_use]
    pub fn window(&self) -> WindowId {
        self.inner.window.get()
    }

    /// The same, without subscribing.
    #[must_use]
    pub fn window_untracked(&self) -> WindowId {
        self.inner.window.get_untracked()
    }

    /// How many times a sink has arrived or gone away. Tracked.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.inner.mounted.get()
    }

    /// How `spot` takes the keyboard, when anything has said.
    #[must_use]
    pub fn sink_for(&self, spot: Spot) -> Option<Sink> {
        self.inner.sinks.borrow().get(&spot).cloned()
    }
}
