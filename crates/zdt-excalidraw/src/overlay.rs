//! What is drawn over the drawing.
//!
//! The selection box, its handles, the band a press drags open and the ghost of a shape not yet
//! drawn. All of it is painted in view pixels with no view box, so a handle is the same size at
//! every zoom — the drawing scales, the chrome does not.

use excalidraw::draw::Piece;
use kurbo::{BezPath, Point, Rect, Shape as _};
use zgui::canvas::zgui_color::Color;
use zgui::canvas::{Brush, CanvasScene, ShapeBuilder};
use zgui::reactive::prelude::*;

use crate::handles::{self, Frame};
use crate::state::{Board, Drag, Live};

/// The blue the editor draws its own marks in.
const ACCENT: Color = Color::srgb(0.35, 0.62, 1.0, 1.0);
/// What a handle is filled with.
const HANDLE_FILL: Color = Color::srgb(1.0, 1.0, 1.0, 1.0);
/// The band a press drags open.
const BAND_FILL: Color = Color::srgb(0.35, 0.62, 1.0, 0.08);
/// How thin the chrome's lines are.
const HAIRLINE: f64 = 1.0;

/// Paints everything the editor draws over the drawing.
pub fn draw(scene: &mut CanvasScene, board: &Board) {
    let live = board.live.get();
    // A ghost of what has not happened yet, under the chrome.
    if let Some(live) = &live {
        ghost(scene, board, live);
    }
    eraser(scene, board);
    if let Some(live) = &live
        && let Drag::Point { .. } = &live.drag
    {
        bending(scene, board, live);
        return;
    }
    frame(scene, board, live.as_ref());
    points(scene, board);
    if let Some(live) = &live
        && matches!(live.drag, Drag::Band)
    {
        band(scene, board, live);
    }
}

/// The line being bent, drawn from the points the drag is holding.
///
/// The line itself is drawn at nothing while this is going on, so this is the only one on the page.
fn bending(scene: &mut CanvasScene, board: &Board, live: &Live) {
    let Drag::Point { id, points, at } = &live.drag else {
        return;
    };
    let mut points = points.clone();
    if let Some(held) = points.get_mut(*at) {
        *held = live.at;
    }

    let held = board.read();
    let Some(element) = held.element(id) else {
        return;
    };
    // Drawn as the element will be, with the points the drag has put it at.
    let mut moved = element.clone();
    let origin = points[0];
    if let excalidraw::element::Data::Linear(linear) = &mut moved.data {
        linear.points = points
            .iter()
            .map(|point| Point::new(point.x - origin.x, point.y - origin.y))
            .collect();
    }
    moved.x = origin.x;
    moved.y = origin.y;
    moved.angle = 0.0;
    drop(held);

    let to_screen = board.viewport.to_screen();
    for piece in excalidraw::draw::pieces(&moved) {
        push(scene, &piece, to_screen, board.dark());
    }
    for (index, point) in points.iter().enumerate() {
        let at_screen = board.viewport.place(*point);
        handle(scene, at_screen, handles::POINT_SIZE, index == *at);
    }
}

/// The handles a chosen line offers: its own points, and the middles that become points.
fn points(scene: &mut CanvasScene, board: &Board) {
    let held = board.read();
    let chosen = held.selection();
    if chosen.len() != 1 {
        return;
    }
    let Some(element) = chosen.first().and_then(|id| held.element(id)) else {
        return;
    };
    if !element.kind.is_linear() {
        return;
    }
    for grip in handles::point_handles(element, &board.viewport) {
        let size = if grip.real {
            handles::POINT_SIZE
        } else {
            handles::MIDPOINT_SIZE
        };
        // A point is a square like every other handle; a middle is a circle, because it is not a
        // point yet.
        handle(scene, grip.at, size, !grip.real);
    }
}

/// How wide the eraser's own pointer is drawn, in view pixels.
const ERASER_SIZE: f64 = 18.0;

/// The eraser's own pointer.
///
/// Drawn rather than asked for: the cursor vocabulary has an arrow, a hand and a crosshair, and
/// nothing that reads as a rubber. So the system pointer is hidden while the eraser is out and this
/// is put where it was.
fn eraser(scene: &mut CanvasScene, board: &Board) {
    if board.tool.get() != crate::state::Tool::Eraser {
        return;
    }
    let Some(at) = board.pointer.get() else {
        return;
    };
    let ring = kurbo::Circle::new(at, ERASER_SIZE / 2.0).to_path(0.1);
    scene.push(
        ShapeBuilder::new(ring.clone())
            .fill(Brush::Solid(HANDLE_FILL))
            .build(),
    );
    stroke(scene, ring, ACCENT, HAIRLINE);
}

/// The box around what is selected, and its handles.
fn frame(scene: &mut CanvasScene, board: &Board, live: Option<&Live>) {
    let Some(frame) = moved_frame(board, live) else {
        return;
    };
    // While a drag is under way the handles would be in the way of it, so only the box is drawn.
    let dragging = live.is_some_and(|live| {
        matches!(
            live.drag,
            Drag::Move | Drag::Resize { .. } | Drag::Rotate { .. }
        )
    });

    let outline = turned(frame.outline(), frame.center(), frame.angle);
    stroke(scene, outline, ACCENT, HAIRLINE);
    if dragging {
        return;
    }
    for (_, at) in frame.scale_handles() {
        handle(scene, at, handles::SIZE, false);
    }
    // The stalk that says which way is up, and the handle that turns it.
    let box_ = frame.outline();
    let mut stalk = BezPath::new();
    stalk.move_to(frame.screen(Point::new(box_.center().x, box_.y0)));
    stalk.line_to(frame.rotation_handle());
    stroke(scene, stalk, ACCENT, HAIRLINE);
    handle(scene, frame.rotation_handle(), handles::SIZE, true);
}

/// What the drag is doing, as a transform in view pixels.
///
/// The chrome is drawn in the view's own pixels, so the drawing's transform is taken over into
/// them: the same change, measured where the handles are.
fn dragging(board: &Board, live: &Live) -> Option<kurbo::Affine> {
    let drag = crate::pointer::transform_of(live)?;
    let to_screen = board.viewport.to_screen();
    Some(to_screen * drag * to_screen.inverse())
}

/// The frame, moved to where the drag has taken it.
fn moved_frame(board: &Board, live: Option<&Live>) -> Option<Frame> {
    let frame = crate::pointer::frame(board)?;
    let Some(live) = live else {
        return Some(frame);
    };
    let drag = dragging(board, live)?;
    // The box the drag leaves it in, taken from the corners so a turn is not lost.
    let corners = [
        frame.box_.origin(),
        Point::new(frame.box_.x1, frame.box_.y0),
        Point::new(frame.box_.x1, frame.box_.y1),
        Point::new(frame.box_.x0, frame.box_.y1),
    ];
    let about = frame.center();
    let upright = |at: Point| excalidraw::geom::rotated(drag * at, drag * about, -frame.angle);
    let mut box_ = Rect::from_points(upright(corners[0]), upright(corners[2]));
    for corner in corners {
        box_ = box_.union_pt(upright(corner));
    }
    let turn = match &live.drag {
        Drag::Rotate { about, start } => crate::pointer::turned(live, *about, *start),
        _ => 0.0,
    };
    Some(Frame {
        box_,
        angle: frame.angle + turn,
        // The point it turns about goes where the drag takes it, like everything else.
        about: drag * about,
    })
}

/// The shape a drag is drawing, as it would look.
///
/// A pen stroke and a run of points are drawn straight from their points rather than through an
/// element: they grow with every movement of the pointer, and building one and reading it back
/// sixty times a second is what makes a long line lag behind the hand drawing it.
fn ghost(scene: &mut CanvasScene, board: &Board, live: &Live) {
    let to_screen = board.viewport.to_screen();
    let dark = board.dark();
    let held = board.read();
    let ink = crate::color::in_scheme(
        &held.style.stroke_color,
        (held.style.opacity / 100.0).clamp(0.0, 1.0),
        dark,
    );
    let chosen_width = held.style.stroke_width;
    drop(held);

    match &live.drag {
        Drag::DrawFree { points, pressures } => {
            if points.len() < 2 {
                return;
            }
            let width = crate::state::Tool::Freedraw.stroke_width(chosen_width);
            let mut path = excalidraw_rough::Stroke {
                points,
                pressures,
                simulate_pressure: true,
                stroke_width: width,
                streamline: excalidraw_rough::freehand::DEFAULT_STREAMLINE,
                variability: excalidraw_rough::Variability::Variable,
            }
            .path();
            path.apply_affine(to_screen);
            scene.push(ShapeBuilder::new(path).fill(Brush::Solid(ink)).build());
        }
        Drag::DrawPoints { points, .. } => {
            let mut walk = points.clone();
            if walk.last().is_none_or(|last| *last != live.at) {
                walk.push(live.at);
            }
            if walk.len() < 2 {
                return;
            }
            let mut path = BezPath::new();
            path.move_to(walk[0]);
            for point in &walk[1..] {
                path.line_to(*point);
            }
            path.apply_affine(to_screen);
            scene.push(
                ShapeBuilder::new(path)
                    .stroke(Brush::Solid(ink), chosen_width * board.viewport.zoom())
                    .build(),
            );
        }
        // A box is one shape however far it is dragged, so it costs the same every time and can be
        // shown exactly as it will be drawn.
        Drag::DrawBox { .. } => {
            let Some(made) = crate::pointer::pending(board, live) else {
                return;
            };
            let Some(element) = made.as_object().and_then(excalidraw::element::read) else {
                return;
            };
            for piece in excalidraw::draw::pieces(&element) {
                push(scene, &piece, to_screen, dark);
            }
        }
        // Everything else moves what is already drawn, and that is shown in the band it is in.
        _ => {}
    }
}

/// One piece of a ghost, taken to the view.
fn push(scene: &mut CanvasScene, piece: &Piece, to_screen: kurbo::Affine, dark: bool) {
    let mut path = BezPath::clone(&piece.path);
    path.apply_affine(to_screen);
    let mut shape = ShapeBuilder::new(path);
    if let Some(fill) = &piece.fill {
        let brush = Brush::Solid(crate::color::in_scheme(&fill.color, fill.alpha, dark));
        shape = if piece.even_odd {
            shape.fill_even_odd(brush)
        } else {
            shape.fill(brush)
        };
    }
    if let Some((paint, stroke)) = &piece.stroke {
        shape = shape.stroke_styled(
            Brush::Solid(crate::color::in_scheme(&paint.color, paint.alpha, dark)),
            stroke.clone(),
        );
    }
    scene.push(shape.build());
}

/// The band a press has dragged open.
fn band(scene: &mut CanvasScene, board: &Board, live: &Live) {
    let from = board.viewport.place(live.from);
    let to = board.viewport.place(live.at);
    let box_ = Rect::from_points(from, to);
    scene.push(
        ShapeBuilder::new(box_.to_path(0.1))
            .fill(Brush::Solid(BAND_FILL))
            .stroke(Brush::Solid(ACCENT), HAIRLINE)
            .build(),
    );
}

/// One handle: a small white mark with an outline, so it shows on any drawing.
fn handle(scene: &mut CanvasScene, at: Point, size: f64, round: bool) {
    let path = if round {
        kurbo::Circle::new(at, size / 2.0).to_path(0.1)
    } else {
        Rect::from_center_size(at, (size, size)).to_path(0.1)
    };
    scene.push(
        ShapeBuilder::new(path.clone())
            .fill(Brush::Solid(HANDLE_FILL))
            .build(),
    );
    stroke(scene, path, ACCENT, HAIRLINE);
}

/// A stroked path in the editor's own colour.
fn stroke(scene: &mut CanvasScene, path: BezPath, color: Color, width: f64) {
    scene.push(
        ShapeBuilder::new(path)
            .stroke(Brush::Solid(color), width)
            .build(),
    );
}

/// `box_` turned `angle` about `about`.
fn turned(box_: Rect, about: Point, angle: f64) -> BezPath {
    let mut path = box_.to_path(0.1);
    path.apply_affine(
        kurbo::Affine::translate(about.to_vec2())
            * kurbo::Affine::rotate(angle)
            * kurbo::Affine::translate(-about.to_vec2()),
    );
    path
}
