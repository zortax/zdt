//! The crop overlay: the kept rectangle, the shaded rest, and the drag that reshapes it.
//!
//! Drawn over the plane, so its coordinate space is the drawn picture's and every position is a
//! percentage. The rectangle is stored on the [`super::edit::Edits`] in the unturned space; the
//! mapping through the pending turns and mirrors happens at the edge, on the way in and out.

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::{component, view};

use super::edit::{Edits, from_display, to_display};
use crate::rich::stage::Camera;

/// How close to an edge a press counts as taking hold of it, in CSS pixels.
const GRIP: f32 = 8.0;

/// The smallest rectangle a drag can leave, as a fraction of the picture.
const SLIGHTEST: f32 = 0.02;

/// What one drag is doing.
#[derive(Clone, Copy)]
struct Drag {
    /// Which part was taken hold of.
    hold: Hold,
    /// Where the pointer went down, in fractions of the plane.
    from: (f32, f32),
    /// The rectangle it found, in the drawn space.
    rect: [f32; 4],
}

/// Which part of the rectangle a drag holds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Hold {
    /// The whole rectangle, moved.
    Body,
    /// An edge or a corner: whether the left, right, top and bottom sides follow.
    Sides {
        left: bool,
        right: bool,
        top: bool,
        bottom: bool,
    },
}

#[component]
pub fn CropOverlay(
    /// The camera, for the plane's drawn size.
    camera: Camera,
    /// The edits the rectangle belongs to.
    edits: Edits,
) -> impl IntoView {
    let node = NodeRef::new();
    let drag: RwSignal<Option<Drag>, LocalStorage> = RwSignal::new_local(None);

    // The pending rectangle in the drawn space, which is the one the overlay draws in.
    let shown = move || {
        let rect = edits.crop().unwrap_or([0.0, 0.0, 1.0, 1.0]);
        let (flip_h, flip_v) = edits.flips();
        to_display(rect, edits.quarter(), flip_h, flip_v)
    };

    // The pointer as fractions of the plane.
    let fraction = move |position: zgui::geom::Point<zgui::geom::CssPx, zgui::geom::Css>| {
        let Some(bounds) = node.window_bounds() else {
            return (0.0, 0.0);
        };
        let scale = crate::rich::stage::density_of(node);
        let width = bounds.size.width.0 / scale;
        let height = bounds.size.height.0 / scale;
        if width <= 0.0 || height <= 0.0 {
            return (0.0, 0.0);
        }
        (
            (position.x.0 - bounds.origin.x.0 / scale) / width,
            (position.y.0 - bounds.origin.y.0 / scale) / height,
        )
    };

    // GRIP, as fractions of each axis.
    let grip = move || {
        let (_, _, width, height) = camera.placement().unwrap_or((0.0, 0.0, 1.0, 1.0));
        (GRIP / width.max(1.0), GRIP / height.max(1.0))
    };

    let percent =
        move |take: fn([f32; 4]) -> f32| move || Some(format!("{}%", take(shown()) * 100.0));

    view! {
        box(
            class = "imgcrop",
            node_ref = node,
            on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                if !ev.primary {
                    return;
                }
                let at = fraction(ev.position);
                let rect = shown();
                let (grip_x, grip_y) = grip();
                let [x, y, w, h] = rect;
                let near = |edge: f32, to: f32, tolerance: f32| (to - edge).abs() <= tolerance;
                let inside_x = at.0 >= x - grip_x && at.0 <= x + w + grip_x;
                let inside_y = at.1 >= y - grip_y && at.1 <= y + h + grip_y;
                if !inside_x || !inside_y {
                    return;
                }
                let left = near(x, at.0, grip_x);
                let right = near(x + w, at.0, grip_x);
                let top = near(y, at.1, grip_y);
                let bottom = near(y + h, at.1, grip_y);
                let hold = if left || right || top || bottom {
                    Hold::Sides { left, right, top, bottom }
                } else {
                    Hold::Body
                };
                drag.set(Some(Drag { hold, from: at, rect }));
                ev.capture_pointer();
                // The stage behind must not also take this as a pan.
                ev.stop_propagation();
            },
            on:pointer_move = move |ev: &mut EventCx<'_, events::PointerMove>| {
                let Some(held) = drag.get_untracked() else {
                    return;
                };
                let at = fraction(ev.position);
                let (dx, dy) = (at.0 - held.from.0, at.1 - held.from.1);
                let [x, y, w, h] = held.rect;
                let next = match held.hold {
                    Hold::Body => [
                        (x + dx).clamp(0.0, 1.0 - w),
                        (y + dy).clamp(0.0, 1.0 - h),
                        w,
                        h,
                    ],
                    Hold::Sides { left, right, top, bottom } => {
                        let mut x1 = if left { (x + dx).clamp(0.0, x + w - SLIGHTEST) } else { x };
                        let mut x2 = if right { (x + w + dx).clamp(x1 + SLIGHTEST, 1.0) } else { x + w };
                        let mut y1 = if top { (y + dy).clamp(0.0, y + h - SLIGHTEST) } else { y };
                        let mut y2 = if bottom { (y + h + dy).clamp(y1 + SLIGHTEST, 1.0) } else { y + h };
                        if x2 < x1 {
                            std::mem::swap(&mut x1, &mut x2);
                        }
                        if y2 < y1 {
                            std::mem::swap(&mut y1, &mut y2);
                        }
                        [x1, y1, x2 - x1, y2 - y1]
                    }
                };
                let (flip_h, flip_v) = edits.flips();
                edits.set_crop(from_display(next, edits.quarter(), flip_h, flip_v));
            },
            on:pointer_up = move |ev: &mut EventCx<'_, events::PointerUp>| {
                drag.set(None);
                ev.release_pointer();
            },
            on:pointer_cancel = move |ev: &mut EventCx<'_, events::PointerCancel>| {
                drag.set(None);
                ev.release_pointer();
            }
        ) {
            // The rest of the picture, shaded on each side of the kept rectangle.
            box(
                class = "imgcrop__shade",
                style:left = "0",
                style:top = "0",
                style:width = "100%",
                style:height = percent(|held| held[1])
            ) {}
            box(
                class = "imgcrop__shade",
                style:left = "0",
                style:top = percent(|held| held[1] + held[3]),
                style:width = "100%",
                style:bottom = "0"
            ) {}
            box(
                class = "imgcrop__shade",
                style:left = "0",
                style:top = percent(|held| held[1]),
                style:width = percent(|held| held[0]),
                style:height = percent(|held| held[3])
            ) {}
            box(
                class = "imgcrop__shade",
                style:left = percent(|held| held[0] + held[2]),
                style:top = percent(|held| held[1]),
                style:right = "0",
                style:height = percent(|held| held[3])
            ) {}
            // The kept rectangle and its grips.
            box(
                class = "imgcrop__rect",
                style:left = percent(|held| held[0]),
                style:top = percent(|held| held[1]),
                style:width = percent(|held| held[2]),
                style:height = percent(|held| held[3])
            ) {
                box(class = "imgcrop__grip", attr:data-at = "nw") {}
                box(class = "imgcrop__grip", attr:data-at = "ne") {}
                box(class = "imgcrop__grip", attr:data-at = "sw") {}
                box(class = "imgcrop__grip", attr:data-at = "se") {}
            }
        }
    }
}
