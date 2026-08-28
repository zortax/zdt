//! The editing overlay: what is drawn over the drawing, and what the pointer does to it.
//!
//! Two layers over the stage. A canvas paints the selection, its handles and the drag ghost in
//! screen space, so handles keep their size at every zoom. A plain box above it takes the
//! pointer; what it does not take falls through to the stage, which pans.

mod hit;
mod nodes;
pub mod paint;
mod select;

use zgui::canvas::zgui_color::Color;
use zgui::canvas::{Brush, ShapeBuilder};
use zgui::elements::kurbo::{self, Shape as _};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::{component, view};

use super::{SvgState, Tool, commit};
use crate::rich::stage::Camera;
use crate::workspace::{BufferId, WindowId, use_workspace};
use paint::PaintPanelProps;

/// How close to a handle a press counts as taking hold of it, in CSS pixels.
const GRIP: f32 = 7.0;

/// Which sides of a box a scale drag carries.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Sides {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

/// What one drag is doing.
#[derive(Clone, Copy)]
enum Drag {
    /// The selected element, moved whole.
    Move,
    /// A box handle, scaling.
    Handle(Sides),
    /// One point of the selected path.
    Anchor { element: usize, which: nodes::Which },
}

/// One live drag: what it holds, where it started, and how far it has come, all in document
/// coordinates.
#[derive(Clone, Copy)]
struct Live {
    drag: Drag,
    from: kurbo::Point,
    delta: kurbo::Vec2,
}

/// The eight handles of a box, with the sides each one drags.
fn handles(bounds: kurbo::Rect) -> [(Sides, kurbo::Point); 8] {
    let side = |left, right, top, bottom| Sides {
        left,
        right,
        top,
        bottom,
    };
    let (cx, cy) = ((bounds.x0 + bounds.x1) / 2.0, (bounds.y0 + bounds.y1) / 2.0);
    [
        (
            side(true, false, true, false),
            kurbo::Point::new(bounds.x0, bounds.y0),
        ),
        (
            side(false, true, true, false),
            kurbo::Point::new(bounds.x1, bounds.y0),
        ),
        (
            side(true, false, false, true),
            kurbo::Point::new(bounds.x0, bounds.y1),
        ),
        (
            side(false, true, false, true),
            kurbo::Point::new(bounds.x1, bounds.y1),
        ),
        (
            side(false, false, true, false),
            kurbo::Point::new(cx, bounds.y0),
        ),
        (
            side(false, false, false, true),
            kurbo::Point::new(cx, bounds.y1),
        ),
        (
            side(true, false, false, false),
            kurbo::Point::new(bounds.x0, cy),
        ),
        (
            side(false, true, false, false),
            kurbo::Point::new(bounds.x1, cy),
        ),
    ]
}

#[component]
pub fn SvgEditor(
    /// Which window it is in.
    window: WindowId,
    /// Which buffer it edits.
    buffer: BufferId,
    /// The preview's state.
    state: SvgState,
    /// The camera on the preview's stage.
    camera: Camera,
) -> impl IntoView {
    use zdt_view::Erase;

    let _ = window;
    let workspace = use_workspace();
    let node = NodeRef::new();
    let live: RwSignal<Option<Live>, LocalStorage> = RwSignal::new_local(None);

    // The document's space onto the overlay's, from the camera and the document's view box.
    let screen = move || -> Option<kurbo::Affine> {
        let (left, top, width, _) = camera.placement()?;
        let view_box = state
            .model
            .with(|model| model.as_ref().map(|held| held.view_box))?;
        if view_box[2] <= 0.0 {
            return None;
        }
        let s = f64::from(width) / view_box[2];
        Some(
            kurbo::Affine::translate((f64::from(left), f64::from(top)))
                * kurbo::Affine::scale(s)
                * kurbo::Affine::translate((-view_box[0], -view_box[1])),
        )
    };

    // The pointer in the overlay's CSS pixels.
    let overlay_point = move |position: zgui::geom::Point<zgui::geom::CssPx, zgui::geom::Css>| {
        let scale = crate::rich::stage::density_of(node);
        let (left, top) = node
            .window_bounds()
            .map(|held| (held.origin.x.0 / scale, held.origin.y.0 / scale))
            .unwrap_or((0.0, 0.0));
        kurbo::Point::new(
            f64::from(position.x.0 - left),
            f64::from(position.y.0 - top),
        )
    };

    let on_down = {
        let workspace = workspace.clone();
        move |ev: &mut EventCx<'_, events::PointerDown>| {
            let _ = &workspace;
            if !ev.primary {
                return;
            }
            let Some(affine) = screen() else {
                return;
            };
            let at_screen = overlay_point(ev.position);
            let at_doc = affine.inverse() * at_screen;
            let Some(model) = state.model.get_untracked() else {
                return;
            };
            let doc_grip = f64::from(GRIP) / hit::scale_of(affine);

            let take = |drag: Drag, ev: &mut EventCx<'_, events::PointerDown>| {
                live.set(Some(Live {
                    drag,
                    from: at_doc,
                    delta: kurbo::Vec2::ZERO,
                }));
                ev.capture_pointer();
                ev.stop_propagation();
            };

            match state.tool.get_untracked() {
                Tool::Select => {
                    if let Some(at) = state.selected.get_untracked()
                        && let Some(held) = model.node(at)
                    {
                        let bounds = affine.transform_rect_bbox(held.bounds());
                        for (sides, corner) in handles(bounds) {
                            if (corner - at_screen).hypot() <= f64::from(GRIP) {
                                take(Drag::Handle(sides), ev);
                                return;
                            }
                        }
                    }
                    match hit::top_most(&model, at_doc, doc_grip) {
                        Some(found) => {
                            state.selected.set(Some(found));
                            take(Drag::Move, ev);
                        }
                        // The press falls through to the stage, which pans.
                        None => state.selected.set(None),
                    }
                }
                Tool::Nodes => {
                    if let Some(at) = state.selected.get_untracked()
                        && let Some(held) = model.node(at)
                    {
                        let to_screen = affine * held.to_doc;
                        for point in nodes::points_of(&held.local) {
                            if (to_screen * point.at - at_screen).hypot() <= f64::from(GRIP) {
                                take(
                                    Drag::Anchor {
                                        element: point.element,
                                        which: point.which,
                                    },
                                    ev,
                                );
                                return;
                            }
                        }
                    }
                    match hit::top_most(&model, at_doc, doc_grip) {
                        Some(found) => {
                            state.selected.set(Some(found));
                            ev.capture_pointer();
                            ev.stop_propagation();
                        }
                        None => state.selected.set(None),
                    }
                }
            }
        }
    };

    let on_move = move |ev: &mut EventCx<'_, events::PointerMove>| {
        let Some(mut held) = live.get_untracked() else {
            return;
        };
        let Some(affine) = screen() else {
            return;
        };
        let at_doc = affine.inverse() * overlay_point(ev.position);
        held.delta = at_doc - held.from;
        live.set(Some(held));
    };

    let on_up = {
        let workspace = workspace.clone();
        move |ev: &mut EventCx<'_, events::PointerUp>| {
            ev.release_pointer();
            let Some(held) = live.get_untracked() else {
                return;
            };
            live.set(None);
            if held.delta.hypot() < 1e-6 {
                return;
            }
            let Some(at) = state.selected.get_untracked() else {
                return;
            };
            let edit = state.model.with_untracked(|model| {
                let model = model.as_ref()?;
                match held.drag {
                    Drag::Move => select::moved(model, at, held.delta),
                    Drag::Handle(sides) => select::scaled(model, at, sides, held.delta),
                    Drag::Anchor { element, which } => {
                        nodes::moved_point(model, at, element, which, held.delta)
                    }
                }
            });
            if let Some(edit) = edit {
                commit(&workspace, buffer, state, edit);
            }
        }
    };

    let overlay = zgui::elements::canvas()
        .class("svgedit__canvas")
        .draw(move |cx| draw(cx, state, camera, live));

    view! {
        box(class = "svgedit") {
            {overlay}
            box(
                class = "svgedit__surface",
                node_ref = node,
                on:pointer_down = on_down,
                on:pointer_move = on_move,
                on:pointer_up = on_up,
                on:pointer_cancel = move |ev: &mut EventCx<'_, events::PointerCancel>| {
                    live.set(None);
                    ev.release_pointer();
                }
            ) {}
            {move || {
                if state.painting.get() {
                    view! { PaintPanel(buffer = buffer, state = state) }.any()
                } else {
                    ().any()
                }
            }}
        }
    }
    .any()
}

/// Paints the selection, its handles and the drag ghost, in screen space.
fn draw(
    cx: &mut zgui::elements::DrawCx<'_>,
    state: SvgState,
    camera: Camera,
    live: RwSignal<Option<Live>, LocalStorage>,
) {
    /// The editor's own blue, over any drawing.
    const ACCENT: Color = Color::srgb(0.35, 0.62, 1.0, 1.0);
    const PAPER: Color = Color::srgb(1.0, 1.0, 1.0, 1.0);
    const SHADOW: Color = Color::srgb(0.0, 0.0, 0.0, 0.6);

    // The same mapping the pointer uses, rebuilt from the tracked signals.
    let Some((left, top, width, _)) = camera.placement() else {
        return;
    };
    let Some(view_box) = state
        .model
        .with(|model| model.as_ref().map(|held| held.view_box))
    else {
        return;
    };
    if view_box[2] <= 0.0 {
        return;
    }
    let affine = kurbo::Affine::translate((f64::from(left), f64::from(top)))
        * kurbo::Affine::scale(f64::from(width) / view_box[2])
        * kurbo::Affine::translate((-view_box[0], -view_box[1]));

    let Some(at) = state.selected.get() else {
        return;
    };
    let tool = state.tool.get();
    let moving = live.get();

    let outline = state.model.with(|model| {
        let node = model.as_ref()?.node(at)?;
        Some((
            node.in_doc(),
            node.local.clone(),
            node.to_doc,
            node.bounds(),
        ))
    });
    let Some((in_doc, local, to_doc, bounds)) = outline else {
        return;
    };

    let stroke =
        |scene: &mut zgui::canvas::CanvasScene, path: kurbo::BezPath, color: Color, width: f64| {
            scene.push(
                ShapeBuilder::new(path)
                    .stroke(Brush::Solid(color), width)
                    .build(),
            );
        };
    let mark = |scene: &mut zgui::canvas::CanvasScene, at: kurbo::Point, size: f64, round: bool| {
        let path = if round {
            kurbo::Circle::new(at, size / 2.0).to_path(0.1)
        } else {
            kurbo::Rect::from_center_size(at, kurbo::Size::new(size, size)).to_path(0.1)
        };
        scene.push(
            ShapeBuilder::new(path.clone())
                .fill(Brush::Solid(PAPER))
                .build(),
        );
        stroke(scene, path, SHADOW, 1.0);
    };

    // The outline, and the ghost of the pending drag over it.
    let mut shown = in_doc.clone();
    match moving.map(|held| (held.drag, held.delta)) {
        Some((Drag::Move, delta)) => shown.apply_affine(kurbo::Affine::translate(delta)),
        Some((Drag::Anchor { element, which }, delta)) => {
            let local_delta = select::linear(to_doc.inverse(), delta);
            let mut moved = nodes::with_moved(&local, element, which, local_delta);
            moved.apply_affine(to_doc);
            shown = moved;
        }
        _ => {}
    }
    shown.apply_affine(affine);
    stroke(cx.scene, shown, ACCENT, 1.5);

    match tool {
        Tool::Select => {
            // The box and its handles, or the box the drag is taking it to.
            let mut screen_bounds = affine.transform_rect_bbox(bounds);
            if let Some(Live {
                drag: Drag::Handle(sides),
                delta,
                ..
            }) = moving
            {
                let mut next = bounds;
                if sides.left {
                    next.x0 += delta.x;
                }
                if sides.right {
                    next.x1 += delta.x;
                }
                if sides.top {
                    next.y0 += delta.y;
                }
                if sides.bottom {
                    next.y1 += delta.y;
                }
                screen_bounds = affine.transform_rect_bbox(next.abs());
            }
            stroke(cx.scene, screen_bounds.to_path(0.1), ACCENT, 1.0);
            for (_, corner) in handles(screen_bounds) {
                mark(cx.scene, corner, 7.0, false);
            }
        }
        Tool::Nodes => {
            // The control cage: each control point tied to its element's ends.
            let to_screen = affine * to_doc;
            let points = nodes::points_of(&local);
            let mut previous_end: Option<kurbo::Point> = None;
            for (element, el) in local.elements().iter().enumerate() {
                use kurbo::PathEl;
                match *el {
                    PathEl::MoveTo(p) | PathEl::LineTo(p) => previous_end = Some(p),
                    PathEl::QuadTo(c, p) => {
                        if let Some(from) = previous_end {
                            let mut cage = kurbo::BezPath::new();
                            cage.move_to(from);
                            cage.line_to(c);
                            cage.line_to(p);
                            cage.apply_affine(to_screen);
                            stroke(cx.scene, cage, SHADOW, 1.0);
                        }
                        previous_end = Some(p);
                    }
                    PathEl::CurveTo(c1, c2, p) => {
                        if let Some(from) = previous_end {
                            let mut cage = kurbo::BezPath::new();
                            cage.move_to(from);
                            cage.line_to(c1);
                            cage.move_to(p);
                            cage.line_to(c2);
                            cage.apply_affine(to_screen);
                            stroke(cx.scene, cage, SHADOW, 1.0);
                        }
                        previous_end = Some(p);
                    }
                    PathEl::ClosePath => {}
                }
                let _ = element;
            }
            for point in points {
                mark(
                    cx.scene,
                    to_screen * point.at,
                    if point.is_anchor() { 7.0 } else { 6.0 },
                    !point.is_anchor(),
                );
            }
        }
    }
}
