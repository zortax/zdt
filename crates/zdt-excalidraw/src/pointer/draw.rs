//! Drawing something new.

use excalidraw::scene::build;
use excalidraw::{Command, Kind};
use kurbo::{Point, Rect};
use zgui::reactive::prelude::*;

use crate::state::{Board, Drag, Live, Tool};

/// The smallest a dragged shape may be before it is treated as a press rather than a drag.
const SMALLEST: f64 = 1.0;

/// What a press with a drawing tool starts.
pub(super) fn down(board: &Board, at: Point, tool: Tool) -> Option<Drag> {
    let kind = tool.kind()?;
    board.with_scene(excalidraw::Scene::clear_selection);
    Some(if tool.walks_points() {
        Drag::DrawPoints {
            kind,
            points: vec![at],
        }
    } else if tool == Tool::Freedraw {
        Drag::DrawFree {
            points: vec![at],
            pressures: vec![0.5],
        }
    } else {
        Drag::DrawBox { kind }
    })
}

/// The element a drag in progress would make, for the overlay to show as a ghost.
#[must_use]
pub fn pending(board: &Board, live: &Live) -> Option<serde_json::Value> {
    made(board, live, false)
}

/// The change a finished drawing makes.
pub(super) fn commit(board: &Board, live: &Live) -> bool {
    let Some(element) = made(board, live, true) else {
        return false;
    };
    let moved = board.apply(Command::Insert(vec![element.clone()]));
    if moved && let Some(id) = element.get("id").and_then(serde_json::Value::as_str) {
        let id = excalidraw::Id::new(id);
        bind_ends(board, &id);
        // A shape is chosen once it is drawn, and the pointer goes back to choosing things, so it
        // can be coloured or moved straight away. A pen stroke does neither: drawing is a run of
        // strokes, and stopping to deselect between them is not how anyone draws.
        if !matches!(kind_of(board, &id), Some(Kind::Freedraw)) {
            board.with_scene(|scene| scene.select([id]));
            board.tool.set(Tool::Select);
        }
    }
    moved
}

/// What kind of thing `id` is, when it is anything.
fn kind_of(board: &Board, id: &excalidraw::Id) -> Option<Kind> {
    board.read_untracked().element(id).map(|held| held.kind)
}

/// Fixes a new arrow's ends to whatever shape they were dropped on.
///
/// An end over nothing stays free. The shape the arrow itself is is never bound to, or an arrow
/// would follow itself.
fn bind_ends(board: &Board, arrow: &excalidraw::Id) {
    let scene = board.read_untracked();
    let Some((_, held)) = scene.drawing.find(arrow) else {
        return;
    };
    if held.kind != Kind::Arrow {
        return;
    }
    let Some(linear) = held.linear() else {
        return;
    };
    let placement = excalidraw::geom::Placement::of(held);
    let ends = [
        (true, placement.scene(linear.points[0])),
        (
            false,
            placement.scene(linear.points[linear.points.len() - 1]),
        ),
    ];

    // Every end's shape is found before anything is written, because writing rereads the drawing.
    let mut bindings = Vec::new();
    for (start, at) in ends {
        let found = scene
            .elements()
            .iter()
            .rev()
            .filter(|shape| shape.id != *arrow && !shape.is_deleted && shape.kind.is_bindable())
            .find(|shape| excalidraw::hit::hits(shape, at, REACH))
            .and_then(|shape| excalidraw::geom::binding::to(shape, at));
        if let Some(binding) = found {
            bindings.push((start, binding));
        }
    }
    drop(scene);

    for (start, binding) in bindings {
        board.apply(Command::Bind {
            arrow: arrow.clone(),
            start,
            to: Some(binding),
        });
    }
}

/// How near a shape an arrow's end has to land to be fixed to it, in scene units.
const REACH: f64 = 12.0;

/// The element `live` describes.
///
/// `fresh` asks for a real id and seed, which only a change that is being kept should take: a ghost
/// asks for the same names every time so it does not walk the generator.
fn made(board: &Board, live: &Live, fresh: bool) -> Option<serde_json::Value> {
    let now = board.read_untracked().now();
    let style = board.style_for(board.tool.get_untracked());

    let (id, seed, nonce) = if fresh {
        let mut id = None;
        let mut seed = None;
        let mut nonce = None;
        board.with_scene(|scene| {
            id = Some(scene.fresh_id());
            seed = Some(scene.fresh_seed());
            nonce = Some(scene.fresh_nonce());
        });
        (id?, seed?, nonce?)
    } else {
        (
            excalidraw::Id::new("pending"),
            excalidraw::element::Seed(1),
            0,
        )
    };

    match &live.drag {
        Drag::DrawBox { kind } => {
            let box_ = live.box_();
            if box_.width() < SMALLEST && box_.height() < SMALLEST {
                return None;
            }
            Some(build::element(
                *kind,
                box_of(box_),
                &style,
                &id,
                seed,
                nonce,
                now,
            ))
        }
        Drag::DrawFree { points, pressures } => {
            if points.len() < 2 {
                return None;
            }
            let mut element = build::element(
                Kind::Freedraw,
                box_of(bounds(points)),
                &style,
                &id,
                seed,
                nonce,
                now,
            );
            put_points(&mut element, points, Some(pressures));
            Some(element)
        }
        Drag::DrawPoints { kind, points } => {
            // The point under the pointer is part of the run while it is still being walked.
            let mut walk = points.clone();
            if walk.last().is_none_or(|last| *last != live.at) {
                walk.push(live.at);
            }
            if walk.len() < 2 {
                return None;
            }
            let mut element =
                build::element(*kind, box_of(bounds(&walk)), &style, &id, seed, nonce, now);
            put_points(&mut element, &walk, None);
            Some(element)
        }
        _ => None,
    }
}

/// The box `points` need.
fn bounds(points: &[Point]) -> Rect {
    let mut held = Rect::from_points(points[0], points[0]);
    for point in points {
        held = held.union_pt(*point);
    }
    held
}

/// A box, as the builder wants it.
const fn box_of(box_: Rect) -> build::Box_ {
    build::Box_ {
        x: box_.x0,
        y: box_.y0,
        width: box_.width(),
        height: box_.height(),
    }
}

/// Writes `points`, taken onto the element's own origin, and the pressures beside them.
fn put_points(element: &mut serde_json::Value, points: &[Point], pressures: Option<&[f64]>) {
    let Some(object) = element.as_object_mut() else {
        return;
    };
    let origin = points[0];
    let local: Vec<Point> = points
        .iter()
        .map(|point| *point - origin.to_vec2())
        .collect();
    // The origin is the first point, so the box follows it.
    object.insert("x".to_owned(), excalidraw::store::Number::json(origin.x));
    object.insert("y".to_owned(), excalidraw::store::Number::json(origin.y));
    object.insert("points".to_owned(), build::points_json(&local));
    if let Some(pressures) = pressures {
        object.insert(
            "pressures".to_owned(),
            serde_json::Value::Array(
                pressures
                    .iter()
                    .map(|held| excalidraw::store::Number::json(*held))
                    .collect(),
            ),
        );
        // The device gave nothing, so the pen's speed decides how hard it pressed.
        object.insert("simulatePressure".to_owned(), serde_json::Value::Bool(true));
    }
}

/// Whether `live` has drawn enough to keep.
#[must_use]
pub fn committed(live: &Live) -> bool {
    match &live.drag {
        Drag::DrawBox { .. } => {
            let box_ = live.box_();
            box_.width() >= SMALLEST || box_.height() >= SMALLEST
        }
        Drag::DrawFree { points, .. } => points.len() >= 2,
        Drag::DrawPoints { points, .. } => !points.is_empty(),
        _ => false,
    }
}
