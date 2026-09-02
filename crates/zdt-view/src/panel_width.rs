//! The width of a panel somebody can drag.
//!
//! A drag writes geometry, and geometry belongs on the element as an inline width: one restyle
//! of one element, and a relayout of the two boxes that share the edge. The setting behind it is
//! written once, when the pointer lets go.

use std::cell::Cell;
use std::rc::Rc;

use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

/// A panel width in CSS pixels, live while it is dragged.
#[derive(Clone)]
pub struct PanelWidth {
    inner: Rc<Inner>,
}

struct Inner {
    /// The width the panel is drawn at.
    live: RwSignal<u32, LocalStorage>,
    /// Whether a drag is in progress. A setting that changes under a drag waits for the release.
    dragging: Cell<bool>,
    narrowest: u32,
    widest: u32,
}

impl PanelWidth {
    /// A width of `start`, kept between `narrowest` and `widest`.
    #[must_use]
    pub fn new(start: u32, narrowest: u32, widest: u32) -> Self {
        Self {
            inner: Rc::new(Inner {
                live: RwSignal::new_local(start.clamp(narrowest, widest)),
                dragging: Cell::new(false),
                narrowest,
                widest,
            }),
        }
    }

    /// The width. Tracked.
    #[must_use]
    pub fn get(&self) -> u32 {
        self.inner.live.get()
    }

    /// The width, without subscribing.
    #[must_use]
    pub fn get_untracked(&self) -> u32 {
        self.inner.live.get_untracked()
    }

    /// The width as an inline length. Tracked.
    #[must_use]
    pub fn px(&self) -> String {
        format!("{}px", self.get())
    }

    /// The width less `by`, as an inline length, for what sits inside the panel. Tracked.
    #[must_use]
    pub fn inset_px(&self, by: u32) -> String {
        format!("{}px", self.get().saturating_sub(by))
    }

    /// Starts a drag. Until [`end`](Self::end), the setting does not move the width.
    pub fn begin(&self) {
        self.inner.dragging.set(true);
    }

    /// Moves the width to `to`, clamped and rounded. Writes only when the pixel changed.
    pub fn drag_to(&self, to: f32) {
        let to = self.clamp(to.round() as u32);
        if self.inner.live.get_untracked() != to {
            self.inner.live.set(to);
        }
    }

    /// Ends a drag, answering the width the setting should keep.
    pub fn end(&self) -> u32 {
        self.inner.dragging.set(false);
        self.inner.live.get_untracked()
    }

    /// Follows the setting, while nothing is being dragged.
    pub fn follow(&self, wanted: u32) {
        let wanted = self.clamp(wanted);
        if !self.inner.dragging.get() && self.inner.live.get_untracked() != wanted {
            self.inner.live.set(wanted);
        }
    }

    fn clamp(&self, width: u32) -> u32 {
        width.clamp(self.inner.narrowest, self.inner.widest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_clamps_and_writes_only_on_change() {
        zgui::reactive::install().ok();
        let scope = zgui::reactive::Mounted::new();
        scope.with(|| {
            let width = PanelWidth::new(260, 160, 480);
            width.begin();
            width.drag_to(1000.0);
            assert_eq!(width.get_untracked(), 480);
            width.drag_to(10.0);
            assert_eq!(width.get_untracked(), 160);
            width.follow(300);
            assert_eq!(
                width.get_untracked(),
                160,
                "a setting waits for the release"
            );
            assert_eq!(width.end(), 160);
            width.follow(300);
            assert_eq!(width.get_untracked(), 300);
        });
    }
}
