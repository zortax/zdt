//! Keeping the row the caret is on inside a virtual list.

use zgui::prelude::*;
use zgui::reactive::RenderEffect;

/// Keeps the row the caret is on inside `port`.
///
/// A virtual list scrolls itself when a pointer asks it to, and knows nothing about a caret moved
/// by a key. Without this, `j` past the bottom of the window moves a selection nobody can see.
///
/// Everything is worked out in device pixels, which is the space a scroll container measures and
/// is scrolled in; the row height is stated in CSS pixels and is converted once, here.
pub(crate) fn keep_visible(
    port: NodeRef,
    at: impl Fn() -> usize + 'static,
    row: f32,
) -> RenderEffect<()> {
    // Observed once, outside the effect: asking for an observation *inside* one registers a fresh
    // observer every time it re-runs, and this effect re-runs on every measurement.
    let measured = port.observe_border_box();

    RenderEffect::new(move |_| {
        let index = at();
        // Read so that the effect follows the container's size as well as the caret.
        let _ = measured.get();

        let position = port.scroll_position();
        let height = position.scrollport.height.0;
        if height <= 0.0 {
            return;
        }
        let scale = port.scale();
        let density = if scale.is_finite() && scale > 0.01 {
            scale
        } else {
            1.0
        };
        let row = row * density;

        let top = position.offset.y.0;
        let wanted = index as f32 * row;
        // Moved as little as possible: a caret walking down scrolls one row at the bottom edge and
        // one walking up scrolls one row at the top. Anything already visible moves nothing, so
        // reading a list with the pointer is not fought by the caret.
        let next = if wanted < top {
            wanted
        } else if wanted + row > top + height {
            wanted + row - height
        } else {
            return;
        };

        port.scroll_to(
            zgui::view::ScrollTarget::Offset(zgui::geom::Point::new(
                zgui::geom::DevicePx(position.offset.x.0),
                zgui::geom::DevicePx(next.max(0.0)),
            )),
            zgui::view::ScrollBehavior::Instant,
        );
    })
}
