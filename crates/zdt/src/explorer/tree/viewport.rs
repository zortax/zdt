//! What the tree can see.
//!
//! The list's scroll container, and the arithmetic over it: how many rows fit, which of them are on
//! screen, where one of them is on the window, and how to bring one back.
//!
//! Every row is the same height and the list is told so, which is what makes all four answerable
//! without measuring a row. The container is the only thing measured.

use std::ops::Range;

use zgui::geom::{Css, CssPx, Device, DevicePx, Point, Rect};
use zgui::prelude::*;
use zgui::reactive::LocalStorage;
use zgui::view::{ScrollBehavior, ScrollPosition, ScrollTarget};

use super::ROW;
use zdt_view::anchor::{AnchorRect, Density};

/// How fast a drag at the very edge of the list scrolls it, in CSS pixels per tick.
///
/// A little under one row per tick at sixty ticks a second, so the list moves quickly enough to
/// reach a distant directory and slowly enough that the row wanted can be stopped on.
const PULL: f32 = 12.0;

/// Which row a point falls on, given where the list starts and how far it is scrolled.
///
/// [`Viewport::row_rect`] read backwards. A free function, so the rounding can be tested without a
/// laid-out window.
fn row_of(top: f32, offset: f32, y: f32) -> Option<usize> {
    let within = y - top + offset;
    (within >= 0.0).then(|| (within / ROW).floor() as usize)
}

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

    /// Which row is under `point`, in CSS pixels from the window's top-left corner. Tracked.
    ///
    /// `None` when the point is outside the list, or past the last of `count` rows. A drag asks
    /// this on every pointer move, so it is arithmetic over the one measured box rather than a
    /// walk of the rows.
    #[must_use]
    pub fn row_at(self, point: Point<CssPx, Css>, count: usize) -> Option<usize> {
        if !self.holds(point) {
            return None;
        }
        let top = self.top()?;
        row_of(top, self.offset(), point.y.0).filter(|at| *at < count)
    }

    /// Whether `point` is inside the list. Tracked.
    #[must_use]
    pub fn holds(self, point: Point<CssPx, Css>) -> bool {
        let Some(box_) = self.window_box() else {
            return false;
        };
        point.x.0 >= box_.x
            && point.x.0 < box_.x + box_.width
            && point.y.0 >= box_.y
            && point.y.0 < box_.y + box_.height
    }

    /// How hard the list should scroll while a drag sits near one of its edges, in CSS pixels per
    /// tick. Tracked.
    ///
    /// Zero away from the edges, negative at the top. One row of margin, so the pull starts while
    /// there is still list under the pointer to aim at.
    #[must_use]
    pub fn pull(self, point: Point<CssPx, Css>) -> f32 {
        let Some(box_) = self.window_box() else {
            return 0.0;
        };
        // Outside sideways is still a pull: a pointer that has wandered off the panel is one that
        // is being dragged somewhere, and the list under it should keep moving.
        let above = box_.y + ROW - point.y.0;
        let below = point.y.0 - (box_.y + box_.height - ROW);
        if above > 0.0 {
            -PULL * (above / ROW).min(1.0)
        } else if below > 0.0 {
            PULL * (below / ROW).min(1.0)
        } else {
            0.0
        }
    }

    /// Scrolls by `dy` CSS pixels, stopping at the ends.
    pub fn nudge(self, dy: f32) {
        let density = Density::reported(self.node.scale());
        let position = self.node.scroll_position();
        let wanted = density.css(position.offset.y.0) + dy;
        let furthest = density.css(position.content_size.height.0 - position.scrollport.height.0);
        self.node.scroll_to(
            ScrollTarget::Offset(Point::new(
                position.offset.x,
                DevicePx(density.device(wanted.clamp(0.0, furthest.max(0.0)))),
            )),
            ScrollBehavior::Instant,
        );
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

    /// The list's box on the window, in CSS pixels. Tracked.
    fn window_box(self) -> Option<AnchorRect> {
        let _ = self.measured.get();
        let bounds = self.node.window_bounds()?;
        let density = Density::reported(self.node.scale());
        Some(AnchorRect {
            x: density.css(bounds.origin.x.0),
            y: density.css(bounds.origin.y.0),
            width: density.css(bounds.size.width.0),
            height: density.css(bounds.size.height.0),
        })
    }

    /// Where the first row would start, on the window. Tracked.
    fn top(self) -> Option<f32> {
        self.window_box().map(|box_| box_.y)
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

#[cfg(test)]
mod tests {
    use super::{ROW, row_of};

    #[test]
    fn a_point_above_the_first_row_is_on_no_row() {
        assert_eq!(row_of(100.0, 0.0, 99.0), None);
    }

    #[test]
    fn the_boundary_between_two_rows_belongs_to_the_lower_one() {
        assert_eq!(row_of(100.0, 0.0, 100.0), Some(0));
        assert_eq!(row_of(100.0, 0.0, 100.0 + ROW - 0.1), Some(0));
        assert_eq!(row_of(100.0, 0.0, 100.0 + ROW), Some(1));
    }

    #[test]
    fn a_scrolled_list_answers_the_row_under_the_pointer() {
        // Three rows scrolled away, so the row drawn at the top of the list is the fourth.
        assert_eq!(row_of(100.0, ROW * 3.0, 100.0), Some(3));
        assert_eq!(row_of(100.0, ROW * 3.0, 100.0 + ROW * 2.0), Some(5));
    }

    #[test]
    fn a_point_above_a_list_scrolled_to_its_start_is_still_on_no_row() {
        assert_eq!(row_of(100.0, 0.0, 80.0), None);
    }
}
