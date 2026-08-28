//! What the pointer does.
//!
//! A press decides what the drag is; a move only records where the pointer is; the release is the
//! one place a change is made. That is what makes one gesture one step of the undo history, and it
//! is what lets the overlay draw a ghost of what has not happened yet.

mod draw;
mod select;

use excalidraw::Command;
use kurbo::Point;

use crate::handles::{Frame, Grip};
use crate::state::{Board, Drag, Live, Sides, Tool};
use zgui::reactive::prelude::*;

pub use self::draw::{committed, pending};
pub use self::select::turned;

/// Which keys were held when the pointer moved.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Held {
    /// Whether shift was down, which constrains the gesture.
    pub shift: bool,
    /// Whether alt was, which works about the middle.
    pub alt: bool,
    /// Whether control or the meta key was, which adds to the selection.
    pub adding: bool,
    /// Whether the space bar was, which pans whatever the tool is.
    pub space: bool,
    /// Whether the press was the middle button, which pans too.
    pub middle: bool,
}

/// How near a shape a press has to be, in view pixels.
const TOLERANCE: f64 = 10.0;

/// How far a press has to travel before it counts as a drag rather than a press.
const DRAG_THRESHOLD: f64 = 4.0;

/// How far apart a pen stroke's points are kept, in view pixels.
const SAMPLE: f64 = 2.0;

/// A press at `at`, in view pixels.
///
/// Answers whether the press was taken. A press that was not falls through to whatever is behind
/// the editor.
pub fn down(board: &Board, at: Point, held: Held) -> bool {
    let scene_at = board.viewport.scene_point(at);
    let tool = board.tool.get_untracked();

    // The hand tool, the space bar under any tool, and the middle button move the view.
    if tool == Tool::Hand || held.space || held.middle {
        begin(
            board,
            Live {
                drag: Drag::Pan {
                    from: board.viewport.scroll_untracked(),
                    at,
                },
                from: scene_at,
                at: scene_at,
                constrained: held.shift,
                from_center: held.alt,
            },
        );
        return true;
    }

    // A press anywhere while words are being typed finishes them, and does nothing else: the
    // press was the reader saying they had finished.
    if crate::text::is_open(board) {
        crate::text::finish(board);
        return true;
    }

    let live = match tool {
        Tool::Select => select::down(board, at, scene_at, held),
        Tool::Eraser => Some(Drag::Erase {
            hit: Vec::new(),
            reach: std::rc::Rc::new(erasable(board)),
        }),
        // Words are placed by one press rather than dragged open: there is no box to drag, and a
        // press that drew nothing would be a tool that does nothing.
        Tool::Text => {
            crate::text::open_at(board, at);
            return true;
        }
        _ => draw::down(board, scene_at, tool),
    };
    let Some(drag) = live else {
        return false;
    };
    begin(
        board,
        Live {
            drag,
            from: scene_at,
            at: scene_at,
            constrained: held.shift,
            from_center: held.alt,
        },
    );
    true
}

/// A generous box around everything the eraser could take away.
///
/// Wider than each thing needs, and cheap: the exact shape of a pen stroke is the drawing of it,
/// and drawing the whole page to start a rub is what this is for.
fn erasable(board: &Board) -> Vec<(excalidraw::Id, kurbo::Rect)> {
    let scene = board.read_untracked();
    scene
        .elements()
        .iter()
        .filter(|element| !element.is_deleted && !element.locked)
        .map(|element| (element.id.clone(), reach_of(element)))
        .collect()
}

/// How far past its own box an element can draw.
///
/// A pen stroke spreads either side of its points by half the width the ink is given, and a round
/// line bulges outside the points it was drawn through. Both are let out generously: a box too
/// large only costs one more question, and a box too small loses a stroke.
fn reach_of(element: &excalidraw::Element) -> kurbo::Rect {
    let placement = excalidraw::geom::Placement::of(element);
    let mut local = kurbo::Rect::new(0.0, 0.0, element.width, element.height);
    // A line and a pen stroke are their points, whatever width and height are written beside them.
    let points = element
        .linear()
        .map(|held| held.points.as_slice())
        .or_else(|| element.freedraw().map(|held| held.points.as_slice()));
    for point in points.unwrap_or_default() {
        local = local.union_pt(*point);
    }

    let to_scene = placement.to_scene();
    let corners = [
        to_scene * local.origin(),
        to_scene * Point::new(local.x1, local.y0),
        to_scene * Point::new(local.x1, local.y1),
        to_scene * Point::new(local.x0, local.y1),
    ];
    let mut box_ = kurbo::Rect::from_points(corners[0], corners[2]);
    for corner in corners {
        box_ = box_.union_pt(corner);
    }

    let bulge = if element.kind.is_linear() {
        box_.width().max(box_.height()) * 0.25
    } else {
        0.0
    };
    let slack = element.stroke_width * 4.0 + 8.0 + bulge;
    box_.inflate(slack, slack)
}

/// Starts `live`, and says whether it is moving what is already drawn.
fn begin(board: &Board, live: Live) {
    board.moving.set(matches!(
        live.drag,
        Drag::Move | Drag::Resize { .. } | Drag::Rotate { .. }
    ));
    board.live.set(Some(live));
}

/// Ends whatever was going on.
fn end(board: &Board) {
    board.live.set(None);
    board.moving.set(false);
    if board.editing_points.get_untracked().is_some() {
        board.editing_points.set(None);
    }
    if !board.erasing.get_untracked().is_empty() {
        board
            .erasing
            .set(std::rc::Rc::new(rustc_hash::FxHashSet::default()));
    }
}

/// The pointer moving to `at`, in view pixels.
pub fn moved(board: &Board, at: Point, held: Held) {
    // Kept whether or not anything is being dragged: the eraser draws its own pointer, and it has
    // to know where the pointer is before a press.
    if board.tool.get_untracked() == Tool::Eraser {
        board.pointer.set(Some(at));
    } else if board.pointer.get_untracked().is_some() {
        board.pointer.set(None);
    }

    let Some(mut live) = board.live.get_untracked() else {
        return;
    };
    let scene_at = board.viewport.scene_point(at);
    live.at = scene_at;
    live.constrained = held.shift;
    live.from_center = held.alt;

    match &mut live.drag {
        // The view is moved as the pointer moves, because there is nothing to show a ghost of.
        //
        // Measured in the view's own pixels against where the press landed. Asking where the
        // pointer is *in the drawing* would ask through the scroll this is setting, and the answer
        // would already have moved.
        Drag::Pan { from, at: pressed } => {
            let zoom = board.viewport.zoom_untracked().max(f64::EPSILON);
            let by = at - *pressed;
            board
                .viewport
                .scroll_to(Point::new(from.0 - by.x / zoom, from.1 - by.y / zoom));
        }
        Drag::DrawFree { points, pressures } => {
            // Only far enough from the last one to be a new one. A pointer reports far more
            // movement than a stroke has shape, and every extra point is walked again on every
            // movement after it.
            let zoom = board.viewport.zoom_untracked().max(f64::EPSILON);
            let apart = SAMPLE / zoom;
            if points
                .last()
                .is_none_or(|last| (scene_at - *last).hypot() >= apart)
            {
                points.push(scene_at);
                pressures.push(0.5);
            }
        }
        Drag::Erase { hit, reach } => {
            let tolerance = TOLERANCE / board.viewport.zoom_untracked().max(f64::EPSILON);
            // Only when the pointer is over the box of something not yet rubbed out is the real
            // question worth asking.
            let near = reach.iter().any(|(id, box_)| {
                box_.inflate(tolerance, tolerance).contains(scene_at) && !hit.contains(id)
            });
            if near {
                let scene = board.read_untracked();
                if let Some(element) = scene.hit(scene_at, tolerance)
                    && !hit.contains(&element.id)
                {
                    hit.push(element.id.clone());
                    drop(scene);
                    board
                        .erasing
                        .set(std::rc::Rc::new(hit.iter().cloned().collect()));
                }
            }
        }
        _ => {}
    }
    board.live.set(Some(live));
}

/// The pointer coming up, which is where a change is made.
///
/// Answers whether anything changed.
pub fn up(board: &Board) -> bool {
    let Some(live) = board.live.get_untracked() else {
        return false;
    };
    // A line is drawn by dragging it open, and a press that went nowhere is the reader asking to
    // walk its points instead. So the release only ends the run when the pointer moved.
    if let Drag::DrawPoints { points, .. } = &live.drag {
        let zoom = board.viewport.zoom_untracked().max(f64::EPSILON);
        let went = live.delta().hypot() * zoom;
        if went < DRAG_THRESHOLD {
            return false;
        }
        if points.len() > 1 {
            // Already walking, and this release is the end of a segment rather than of the line.
            return false;
        }
    }
    end(board);

    match &live.drag {
        // The view has already moved, and moving the view is not a change to the drawing.
        Drag::Pan { .. } => false,
        Drag::Band => {
            select::band(board, &live);
            false
        }
        Drag::Move | Drag::Resize { .. } | Drag::Rotate { .. } | Drag::Point { .. } => {
            select::commit(board, &live)
        }
        Drag::Erase { hit, .. } if !hit.is_empty() => board.apply(Command::Delete(hit.clone())),
        Drag::Erase { .. } => false,
        // The tool stays chosen: drawing three boxes should not mean choosing the box tool three
        // times.
        Drag::DrawBox { .. } | Drag::DrawFree { .. } | Drag::DrawPoints { .. } => {
            draw::commit(board, &live)
        }
    }
}

/// A press adding one more point to a run being walked.
///
/// Answers whether the run was finished by it.
pub fn add_point(board: &Board, at: Point) -> bool {
    let Some(mut live) = board.live.get_untracked() else {
        return false;
    };
    let Drag::DrawPoints { points, .. } = &mut live.drag else {
        return false;
    };
    let scene_at = board.viewport.scene_point(at);
    // A point on top of the last one is the reader saying they have finished.
    let zoom = board.viewport.zoom_untracked().max(f64::EPSILON);
    if let Some(last) = points.last()
        && (*last - scene_at).hypot() * zoom < CONFIRM_THRESHOLD
    {
        return finish_points(board);
    }
    points.push(scene_at);
    live.at = scene_at;
    board.live.set(Some(live));
    false
}

/// How near its last point a press has to be to finish a run, in view pixels.
const CONFIRM_THRESHOLD: f64 = 8.0;

/// Finishes a run of points, and answers whether anything was drawn.
pub fn finish_points(board: &Board) -> bool {
    let Some(live) = board.live.get_untracked() else {
        return false;
    };
    if !matches!(live.drag, Drag::DrawPoints { .. }) {
        return false;
    }
    end(board);
    draw::commit(board, &live)
}

/// Gives up whatever the pointer was doing.
pub fn cancel(board: &Board) {
    end(board);
}

/// What a drag is holding, when it is holding anything.
///
/// Only a drag that moves what is already there: a band or a new shape holds nothing that is drawn
/// twice.
#[must_use]
pub fn dragged(board: &Board) -> Vec<excalidraw::Id> {
    let Some(live) = board.live.get() else {
        return Vec::new();
    };
    if !matches!(
        live.drag,
        Drag::Move | Drag::Resize { .. } | Drag::Rotate { .. }
    ) {
        return Vec::new();
    }
    board.read().selection().to_vec()
}

/// Whether `id` is being carried by the drag under way.
///
/// Words written in a shape are carried with it, because that is where they are drawn. A drag that
/// moves nothing — a pan, a band, a new shape — carries nothing, which is what keeps the drawing
/// from being painted again while one is going on.
#[must_use]
pub fn is_dragged(board: &Board, id: &excalidraw::Id) -> bool {
    if !board.moving.get() {
        return false;
    }
    let scene = board.read();
    scene.is_selected(id)
        || scene
            .element(id)
            .and_then(|held| held.text())
            .and_then(|words| words.container_id.as_ref())
            .is_some_and(|container| scene.is_selected(container))
}

/// What the drag under way is doing, as a transform in the drawing's own units.
///
/// Nothing while no drag is moving what is already there. This is what shows a change before it has
/// been made: every band paints what it is holding through this, so a moved shape and the words in
/// it stay in the order they were drawn in.
///
/// Nothing here asks where the view is looking. That is the whole point: a band that read the
/// viewport would be drawn again on every pan, and the drawing is painted in its own coordinates
/// exactly so that it does not have to be.
#[must_use]
pub fn drag_transform(board: &Board) -> Option<kurbo::Affine> {
    transform_of(&board.live.get()?)
}

/// The same, for a drag already in hand.
#[must_use]
pub fn transform_of(live: &Live) -> Option<kurbo::Affine> {
    match &live.drag {
        Drag::Move => Some(kurbo::Affine::translate(live.delta())),
        Drag::Rotate { about, start } => {
            let by = select::turned(live, *about, *start);
            Some(about_point(*about, kurbo::Affine::rotate(by)))
        }
        Drag::Resize {
            sides,
            from,
            angle,
            about,
        } => {
            if from.width() <= 0.0 || from.height() <= 0.0 {
                return None;
            }
            // In the selection's own upright space, so a turned one scales along its own edges.
            let upright = about_point(*about, kurbo::Affine::rotate(-*angle));
            let start = upright * live.from;
            let now = upright * live.at;
            let to = crate::handles::resized(
                *from,
                *sides,
                now - start,
                live.constrained,
                live.from_center,
            );
            let scale = kurbo::Affine::translate(to.origin().to_vec2())
                * kurbo::Affine::scale_non_uniform(
                    to.width() / from.width(),
                    to.height() / from.height(),
                )
                * kurbo::Affine::translate(-from.origin().to_vec2());
            Some(upright.inverse() * scale * upright)
        }
        _ => None,
    }
}

/// `what`, done about `at` rather than about the origin.
fn about_point(at: Point, what: kurbo::Affine) -> kurbo::Affine {
    kurbo::Affine::translate(at.to_vec2()) * what * kurbo::Affine::translate(-at.to_vec2())
}

/// The frame around what is selected, when anything is.
#[must_use]
pub fn frame(board: &Board) -> Option<Frame> {
    let scene = board.read();
    let selected: Vec<&excalidraw::Element> = scene.selected().collect();
    Frame::of(&selected, &board.viewport)
}

/// The same, without subscribing.
#[must_use]
pub fn frame_untracked(board: &Board) -> Option<Frame> {
    let scene = board.read_untracked();
    let selected: Vec<&excalidraw::Element> = scene.selected().collect();
    Frame::of(&selected, &board.viewport)
}

/// Which handle `at` takes hold of, and what dragging it does.
///
/// A turn comes back with nothing for the point it turns about: only the caller knows the drawing,
/// and a turn is about a point in it rather than a point on the screen.
#[must_use]
pub(crate) fn grip_drag(board: &Board, frame: &Frame, grip: Grip) -> Drag {
    match grip {
        Grip::Scale(sides) => {
            let zoom = board.viewport.zoom_untracked().max(f64::EPSILON);
            let origin = board.viewport.scene_point(frame.box_.origin());
            Drag::Resize {
                sides,
                from: kurbo::Rect::new(
                    origin.x,
                    origin.y,
                    origin.x + frame.box_.width() / zoom,
                    origin.y + frame.box_.height() / zoom,
                ),
                angle: frame.angle,
                about: board.viewport.scene_point(frame.center()),
            }
        }
        Grip::Rotate => Drag::Rotate {
            about: Point::ZERO,
            start: 0.0,
        },
    }
}

/// The sides a handle at a corner moves.
#[must_use]
pub const fn corner(left: bool, right: bool, top: bool, bottom: bool) -> Sides {
    Sides {
        left,
        right,
        top,
        bottom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Sides;

    fn board() -> Board {
        let drawing =
            excalidraw::file::parse(r#"{"type":"excalidraw","elements":[]}"#).expect("a drawing");
        Board::new(excalidraw::Scene::new(drawing, 1, 1))
    }

    fn live(drag: Drag) -> Live {
        Live {
            drag,
            from: Point::new(100.0, 100.0),
            at: Point::new(160.0, 140.0),
            constrained: false,
            from_center: false,
        }
    }

    /// The one property that keeps a pan cheap: what a drag is doing is measured in the drawing,
    /// so a band that paints it does not have to know where the view is looking — and is therefore
    /// not painted again every time the view moves.
    #[test]
    fn what_a_drag_is_doing_does_not_depend_on_the_view() {
        let moves = [
            Drag::Move,
            Drag::Rotate {
                about: Point::new(50.0, 50.0),
                start: 0.3,
            },
            Drag::Resize {
                sides: Sides {
                    left: false,
                    right: true,
                    top: false,
                    bottom: true,
                },
                from: kurbo::Rect::new(0.0, 0.0, 100.0, 80.0),
                angle: 0.4,
                about: Point::new(50.0, 40.0),
            },
        ];
        for drag in moves {
            let held = live(drag);
            let at = Point::new(17.0, 23.0);
            let once = transform_of(&held).expect("a transform") * at;
            // Nothing about the view is an input, so there is nothing to change it with. Asked
            // again, it answers the same.
            let again = transform_of(&held).expect("a transform") * at;
            assert_eq!(once, again);
        }
    }

    /// The one thing a pan has to get right: the drawing keeps up with the pointer exactly.
    ///
    /// Measuring the movement through the scroll it is itself setting makes the view chase its own
    /// tail — it falls behind the pointer and snaps about between frames. So this walks several
    /// moves and checks the total, which is where that mistake shows.
    #[test]
    fn a_pan_moves_the_view_exactly_as_far_as_the_pointer_went() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let board = board();
            board.viewport.set_size(800.0, 600.0);
            board.viewport.zoom_to(2.0);
            let zoom = board.viewport.zoom_untracked();
            let start = board.viewport.scroll_untracked();

            board.tool.set(Tool::Hand);
            let held = Held::default();
            let press = Point::new(400.0, 300.0);
            assert!(down(&board, press, held));

            // Several moves, because one alone would hide a pan that measures each against the
            // last rather than against the press.
            let mut last = press;
            for step in 1..=5 {
                last = Point::new(
                    press.x - f64::from(step) * 20.0,
                    press.y - f64::from(step) * 10.0,
                );
                moved(&board, last, held);
            }
            up(&board);

            let went = last - press;
            let now = board.viewport.scroll_untracked();
            assert!(
                (now.0 - (start.0 - went.x / zoom)).abs() < 1e-9,
                "across: {} against {}",
                now.0,
                start.0 - went.x / zoom
            );
            assert!(
                (now.1 - (start.1 - went.y / zoom)).abs() < 1e-9,
                "down: {} against {}",
                now.1,
                start.1 - went.y / zoom
            );
        });
    }

    /// And the point the pointer took hold of stays under it the whole way.
    #[test]
    fn a_pan_keeps_the_same_place_under_the_pointer() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let board = board();
            board.viewport.set_size(800.0, 600.0);
            board.viewport.zoom_to(1.5);

            board.tool.set(Tool::Hand);
            let held = Held::default();
            let press = Point::new(300.0, 200.0);
            let taken = board.viewport.scene_point(press);
            assert!(down(&board, press, held));

            for step in 1..=4 {
                let at = Point::new(
                    press.x + f64::from(step) * 30.0,
                    press.y + f64::from(step) * 15.0,
                );
                moved(&board, at, held);
                let under = board.viewport.scene_point(at);
                assert!(
                    (under - taken).hypot() < 1e-9,
                    "step {step}: {under:?} is no longer {taken:?}"
                );
            }
        });
    }

    #[test]
    fn a_drag_that_moves_nothing_has_no_transform() {
        for drag in [
            Drag::Pan {
                from: (0.0, 0.0),
                at: Point::ZERO,
            },
            Drag::Band,
            Drag::DrawBox {
                kind: excalidraw::Kind::Rectangle,
            },
        ] {
            assert!(transform_of(&live(drag)).is_none());
        }
    }

    #[test]
    fn moving_something_moves_it_by_how_far_the_pointer_came() {
        let held = live(Drag::Move);
        let moved = transform_of(&held).expect("a transform") * Point::ZERO;
        assert!((moved - held.delta().to_point()).hypot() < 1e-9);
    }

    #[test]
    fn scaling_a_box_takes_it_onto_the_one_the_drag_asked_for() {
        let from = kurbo::Rect::new(0.0, 0.0, 100.0, 100.0);
        let held = Live {
            drag: Drag::Resize {
                sides: Sides {
                    left: false,
                    right: true,
                    top: false,
                    bottom: false,
                },
                from,
                angle: 0.0,
                about: from.center(),
            },
            from: Point::new(100.0, 50.0),
            at: Point::new(150.0, 50.0),
            constrained: false,
            from_center: false,
        };
        let drag = transform_of(&held).expect("a transform");
        let to = drag.transform_rect_bbox(from);
        assert!(
            (to.width() - 150.0).abs() < 1e-9,
            "it is {} wide",
            to.width()
        );
        assert!((to.height() - 100.0).abs() < 1e-9, "and no taller");
    }
}
