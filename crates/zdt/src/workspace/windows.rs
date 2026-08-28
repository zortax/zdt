//! Splitting windows, closing them, and moving between them.

use super::*;

impl Workspace {
    // ---- Windows ---------------------------------------------------------------------------

    /// Splits the focused window along `axis`, showing the same buffer in both.
    pub fn split(&self, axis: Axis) -> Option<WindowId> {
        let focused = self.focused_untracked();
        let current = self.buffer_in_untracked(focused)?;
        let new = self
            .inner
            .windows
            .try_update(|windows| {
                // The new split shows what the old one shows, in the same form.
                let carried = |held: &Vec<BufferId>| {
                    held.contains(&current)
                        .then_some(current)
                        .into_iter()
                        .collect()
                };
                let (rich, plain) = windows
                    .get(focused)
                    .map(|state| (carried(&state.rich), carried(&state.plain)))
                    .unwrap_or_default();
                windows.insert(WindowState {
                    current: Some(current),
                    mounted: vec![current],
                    font_step: 0,
                    rich,
                    plain,
                })
            })
            .expect("the window map is writable");
        let split = self
            .inner
            .layout
            .try_update(|layout| layout.split(focused, axis, new))
            .unwrap_or(false);
        if !split {
            self.inner.windows.update(|windows| {
                windows.remove(new);
            });
            return None;
        }
        // The keyboard goes into the new split, wherever it was. Splitting from the tree is asking
        // for the split.
        self.inner.focus.enter_window(new);
        Some(new)
    }

    /// Closes the focused window, unless it is the only one.
    pub fn close_window(&self) -> bool {
        let focused = self.focused_untracked();
        let closed = self
            .inner
            .layout
            .try_update(|layout| layout.close(focused))
            .unwrap_or(false);
        if !closed {
            return false;
        }
        self.inner.windows.update(|windows| {
            windows.remove(focused);
        });
        if let Some(next) = self.inner.layout.get_untracked().windows().first().copied() {
            self.inner.focus.enter_window(next);
        }
        true
    }

    /// Closes `window`, whichever it is.
    ///
    /// The last one stays: a workspace with no window in it has nowhere to put the next buffer.
    pub fn close_window_at(&self, window: WindowId) -> bool {
        let closed = self
            .inner
            .layout
            .try_update(|layout| layout.close(window))
            .unwrap_or(false);
        if !closed {
            return false;
        }
        self.inner.windows.update(|windows| {
            windows.remove(window);
        });
        if self.focused_untracked() == window
            && let Some(next) = self.inner.layout.get_untracked().windows().first().copied()
        {
            // The current pane moves and the keyboard stays where it is: closing a split from the
            // tree must not pull somebody out of the tree.
            self.inner.focus.set_window(next);
        }
        true
    }

    /// Gives the keyboard to `window`.
    pub fn focus_window(&self, window: WindowId) {
        self.inner.focus.enter_window(window);
    }

    /// Gives the keyboard to the next window in the walking order.
    pub fn cycle_window(&self, forward: bool) {
        let layout = self.inner.layout.get_untracked();
        let focused = self.focused_untracked();
        let next = if forward {
            layout.next_after(focused)
        } else {
            layout.previous_before(focused)
        };
        if let Some(next) = next {
            self.inner.focus.enter_window(next);
        }
    }

    /// Writes the sizes a dragged handle reported into the split it belongs to.
    pub fn resize(&self, window: WindowId, sizes: &[f64]) {
        self.inner.layout.update(|layout| {
            layout.resize(window, sizes);
        });
    }
}
