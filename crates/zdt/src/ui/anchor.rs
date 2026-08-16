//! Putting a surface beside the caret.
//!
//! Four things in this editor are anchored to a place in the text rather than to a control: the
//! documentation panel, the completion popup, the panel of documentation beside it, and the
//! signature help. None of them can use the component library's [`Popover`], because a popover is
//! anchored to its own trigger and a caret is not an element — there is nothing with a handle to
//! point at.
//!
//! What they can use is the solver underneath it. [`zgui_ui_primitives::popper::solve`] is a pure
//! function over three rectangles, so the caret's rectangle serves as the anchor and everything a
//! popover gets — flipping to the other side when there is no room, sliding along the edge to stay
//! inside the window, a `data-side` for the style sheet to select the animation on — comes with it.
//!
//! Writing it out by hand instead would be four copies of the same arithmetic, and the first one
//! to be got wrong would be the one that puts a panel of documentation off the bottom of a maximised
//! window, where nobody testing on a small one would ever see it.

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui_ui_primitives::popper::{Placement, PopperOptions, WindowRect, solve};

/// How many device pixels one CSS pixel is on the surface being placed on.
///
/// The measurements a placement is made from — the surface's box, the window's — arrive in device
/// pixels, because that is the space layout is resolved in. The answer is written back as an
/// inline `left` and `top`, and a length in a style sheet is a CSS pixel. The two are the same
/// number only at one device pixel per CSS pixel, which is the display nearly every test runs on
/// and the reason confusing them is invisible until somebody opens the window on a denser output —
/// where a panel four hundred pixels down the file opens a hundred pixels below where it belongs.
///
/// The library has this type and keeps it to itself, so here is the same eight lines.
#[derive(Clone, Copy)]
struct Density(f32);

impl Density {
    /// The density an element reports, made safe to divide by: a density of nothing would divide a
    /// placement into infinity and put the surface nowhere.
    fn reported(scale: f32) -> Self {
        Self(if scale.is_finite() && scale > 0.01 {
            scale
        } else {
            1.0
        })
    }

    /// `css` CSS pixels as device pixels, which is the space a placement is solved in.
    fn device(self, css: f32) -> f32 {
        css * self.0
    }

    /// `device` device pixels as CSS pixels, which is the space an inline length is read in.
    fn css(self, device: f32) -> f32 {
        device / self.0
    }
}

/// Where a surface ended up, as the things a view needs to draw it.
#[derive(Clone, Copy)]
pub struct Placed {
    /// Its left edge, in CSS pixels, once it has been measured.
    pub left: Signal<Option<f32>, LocalStorage>,
    /// Its top edge.
    pub top: Signal<Option<f32>, LocalStorage>,
    /// Which side of the caret it went on, for the style sheet to animate from.
    pub side: Signal<Option<String>, LocalStorage>,
    /// Whether it has been placed at all.
    ///
    /// False for exactly one frame — the one in which the surface is in the tree so that it can be
    /// measured, and has not yet been. A surface drawn in that frame appears at the window's
    /// corner and jumps, which is the one artefact this whole module exists to remove.
    pub settled: Signal<bool, LocalStorage>,
}

/// How a surface should sit against the caret.
#[derive(Clone, Copy)]
pub struct Anchoring {
    /// Where it is asked to go.
    pub placement: Placement,
    /// How far off the caret it sits, in CSS pixels.
    pub offset: f32,
    /// How close to the window's edge it may come.
    pub padding: f32,
}

impl Default for Anchoring {
    fn default() -> Self {
        Self {
            placement: Placement::BOTTOM,
            // Two pixels: enough that the surface is not touching the character it is about,
            // little enough that the two still read as one thing.
            offset: 2.0,
            padding: 6.0,
        }
    }
}

impl Anchoring {
    /// The same, on a given side.
    #[must_use]
    pub fn on(placement: Placement) -> Self {
        Self {
            placement,
            ..Self::default()
        }
    }

    /// The same, at a given distance.
    #[must_use]
    pub const fn offset(mut self, offset: f32) -> Self {
        self.offset = offset;
        self
    }
}

/// Places `surface` against `caret`, and keeps it placed.
///
/// `caret` is read inside the tracked closure, so a surface that follows a moving caret is one
/// that passes a signal rather than one that remounts.
///
/// The measurements are the surface's own border box and the window's, both observed rather than
/// read once: a panel whose text arrives a frame later is a panel that changes size, and one
/// placed against its first size would hang off the bottom of the window from then on.
pub fn place(
    surface: NodeRef,
    caret: impl Fn() -> Option<zgui_editor::CaretRect> + 'static,
    anchoring: Anchoring,
) -> Placed {
    // The window's rectangle, which is what "inside the window" is measured against. Acquired from
    // an effect because the root is only reachable through a bound handle, and taken under the
    // owner captured here: a render effect disposes its own scope on every run, so an observation
    // started inside the closure would be given back the next time anything re-ran it.
    let owner = zgui::reactive::Owner::current();
    let viewport: RwSignal<Option<Signal<Option<WindowRect>, LocalStorage>>, LocalStorage> =
        RwSignal::new_local(None);
    let watching = zgui::reactive::RenderEffect::new(move |_| {
        if surface.get().is_none() || viewport.get_untracked().is_some() {
            return;
        }
        // Both the handle and the observation belong to the placement, not to this run.
        let acquired = {
            let take = move || Some(surface.window_root()?.observe_border_box());
            match &owner {
                Some(owner) => owner.with(take),
                None => take(),
            }
        };
        if acquired.is_some() {
            viewport.set(acquired);
        }
    });
    on_cleanup_local(move || drop(watching));

    let measured = surface.observe_border_box();

    // Stored, because both the solver and the fallback below read it.
    let caret_of = Signal::derive_local(caret);

    let solution = Signal::derive_local(move || {
        let caret = caret_of.get()?;
        let floating = measured.get()?.size;
        // Before the first measurement the surface has no size, and a solution computed from
        // nothing places it against the caret and then moves it.
        if floating.width.0 <= 0.0 && floating.height.0 <= 0.0 {
            return None;
        }
        // `try_get`, so a disposed observation degrades to the rough placement below rather than
        // panicking in the middle of a teardown, where a panic cannot unwind.
        let viewport = viewport.get()?.try_get()??;

        // The caret's rectangle comes from the editor in CSS pixels; the other two measurements
        // are in device pixels. Everything is converted into device space, which is the space the
        // answer is wanted in.
        let density = Density::reported(surface.scale());
        let anchor = WindowRect::new(
            zgui::geom::Point::new(
                zgui::geom::DevicePx(density.device(caret.x)),
                zgui::geom::DevicePx(density.device(caret.y)),
            ),
            zgui::geom::Size::new(
                zgui::geom::DevicePx(density.device(caret.width)),
                zgui::geom::DevicePx(density.device(caret.height)),
            ),
        );

        Some(solve(
            anchor,
            floating,
            viewport,
            &PopperOptions {
                placement: anchoring.placement,
                flip: true,
                shift: true,
                offset: density.device(anchoring.offset),
                padding: density.device(anchoring.padding),
            },
        ))
    });

    // Back into CSS pixels, snapped to a whole device pixel: an edge that falls between two pixels
    // is an edge with a soft border and blurred text on one side of it.
    //
    // A surface that cannot be solved yet is placed *anyway*, under the caret, rather than being
    // left without a position. This is the difference between a panel that jumps once on the frame
    // it opens and a panel that never appears at all: the solver needs three measurements, and if
    // any of them never arrives — a window whose root reports no box, a surface measured as
    // nothing — then a placement that waits for all three waits for ever, invisibly. Preferring a
    // rough answer to no answer is the whole of this fallback.
    let placed = Signal::derive_local(move || {
        let caret = caret_of.get()?;
        let density = Density::reported(surface.scale());

        let Some(solved) = solution.get() else {
            return Some((
                caret.x,
                caret.y + caret.height + anchoring.offset,
                anchoring.placement.side.name().to_owned(),
                false,
            ));
        };
        Some((
            density.css(solved.origin.x.0.round()),
            density.css(solved.origin.y.0.round()),
            solved.placement.side.name().to_owned(),
            true,
        ))
    });

    Placed {
        left: Signal::derive_local(move || placed.get().map(|(x, ..)| x)),
        top: Signal::derive_local(move || placed.get().map(|(_, y, ..)| y)),
        side: Signal::derive_local(move || placed.get().map(|(.., side, _)| side)),
        settled: Signal::derive_local(move || placed.get().is_some()),
    }
}

impl Placed {
    /// The left edge as an inline length.
    pub fn left_px(self) -> impl Fn() -> Option<String> + Clone + 'static {
        let left = self.left;
        move || left.get().map(|x| format!("{x}px"))
    }

    /// The top edge as an inline length.
    pub fn top_px(self) -> impl Fn() -> Option<String> + Clone + 'static {
        let top = self.top;
        move || top.get().map(|y| format!("{y}px"))
    }

    /// `hidden` only while there is nothing to place at all.
    ///
    /// Hidden rather than absent: an element that is not in the tree has no size, and its size is
    /// what decides where it goes. This keeps the box and its layout and takes it out of the paint.
    ///
    /// Note what this does *not* wait for: a solved placement. A surface with a caret to sit under
    /// is shown at once, roughly placed, and moved when the measurements arrive.
    pub fn visibility(self) -> impl Fn() -> Option<String> + Clone + 'static {
        let settled = self.settled;
        move || (!settled.get()).then(|| "hidden".to_owned())
    }
}
