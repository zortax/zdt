//! What the tree can see.
//!
//! The list's scroll container, and the arithmetic over it: how many rows fit, which of them are on
//! screen, where one of them is on the window, and how to bring one back.
//!
//! Every row is the same height and the list is told so, which is what makes all four answerable
//! without measuring a row. The container is the only thing measured.

use std::ops::Range;

use zgui::geom::{Device, DevicePx, Point, Rect};
use zgui::prelude::*;
use zgui::reactive::LocalStorage;
use zgui::view::{ScrollBehavior, ScrollPosition, ScrollTarget};

use super::ROW;
use zdt_view::anchor::{AnchorRect, Density};

/// The list's scroll container, and what it can answer about the rows in it.
///
/// Cloning one is cloning a handle.
#[derive(Clone, Copy)]
pub struct Viewport {
    /// The scroll container the rows are in.
    node: NodeRef,
    /// Its offset and extent. Observed, so everything derived from it follows a scroll.
    scroll: Signal<ScrollPosition, LocalStorage>,
    /// Its box. Observed for the size, and so that a panel somebody widened works itself out
    /// again.
    measured: Signal<Option<Rect<DevicePx, Device>>, LocalStorage>,
}

impl Viewport {
    /// A viewport over `node`, the list's scroll container.
    ///
    /// Made in the panel's own scope, so the two observations live as long as it does.
    #[must_use]
    pub fn new(node: NodeRef) -> Self {
        Self {
            node,
            scroll: node.observe_scroll(),
            measured: node.observe_border_box(),
        }
    }

    /// The container, for the list to bind.
    #[must_use]
    pub fn node(self) -> NodeRef {
        self.node
    }

    /// How many whole rows fit. At least one. Tracked.
    #[must_use]
    pub fn rows(self) -> usize {
        let port = self.port();
        if port <= 0.0 {
            return 1;
        }
        ((port / ROW).floor() as usize).max(1)
    }

    /// How far `<C-d>` and `<C-u>` go. At least one. Tracked.
    #[must_use]
    pub fn half_page(self) -> usize {
        (self.rows() / 2).max(1)
    }

    /// Which rows are on screen. Tracked.
    ///
    /// Part rows included: a row half over the bottom edge is one a person can read a label on.
    /// Unclamped at the far end, so the caller cuts it to the length of the list.
    #[must_use]
    pub fn visible(self) -> Range<usize> {
        let port = self.port();
        if port <= 0.0 {
            return 0..0;
        }
        let offset = self.offset();
        let first = (offset / ROW).floor().max(0.0) as usize;
        let last = ((offset + port) / ROW).ceil().max(0.0) as usize;
        first..last
    }

    /// Where row `at` is on the window, in CSS pixels. Tracked.
    ///
    /// `None` while the panel has not been laid out. A row that has scrolled away answers a
    /// rectangle outside the panel, which the placement solver slides back inside the window.
    #[must_use]
    pub fn row_rect(self, at: usize) -> Option<AnchorRect> {
        // Both subscriptions first. A scroll and a resize are the two things that move a row, and
        // the window box below reports the last frame rather than announcing itself.
        let offset = self.offset();
        let _ = self.measured.get();

        let bounds = self.node.window_bounds()?;
        let density = Density::reported(self.node.scale());
        Some(AnchorRect {
            x: density.css(bounds.origin.x.0),
            y: density.css(bounds.origin.y.0) - offset + at as f32 * ROW,
            width: density.css(bounds.size.width.0),
            height: ROW,
        })
    }

    /// Brings row `at` into view, moving as little as it can.
    ///
    /// In one frame, and never smoothly. A key held down against a smooth scroll leaves the caret
    /// ahead of the rows it is walking.
    pub fn keep_in_view(self, at: usize) {
        let density = Density::reported(self.node.scale());
        let position = self.node.scroll_position();
        let port = density.css(position.scrollport.height.0);
        // Before the first layout there is no port, and a scroll worked out from nothing would
        // move the list on the frame it is built.
        if port <= 0.0 {
            return;
        }

        let offset = density.css(position.offset.y.0);
        let top = at as f32 * ROW;
        let wanted = if top < offset {
            top
        } else if top + ROW > offset + port {
            top + ROW - port
        } else {
            return;
        };

        self.node.scroll_to(
            ScrollTarget::Offset(Point::new(
                position.offset.x,
                DevicePx(density.device(wanted.max(0.0))),
            )),
            ScrollBehavior::Instant,
        );
    }

    /// How tall the visible part is, in CSS pixels. Tracked.
    fn port(self) -> f32 {
        let position = self.scroll.get();
        Density::reported(self.node.scale()).css(position.scrollport.height.0)
    }

    /// How far the list has been scrolled, in CSS pixels. Tracked.
    fn offset(self) -> f32 {
        let position = self.scroll.get();
        Density::reported(self.node.scale()).css(position.offset.y.0)
    }
}
