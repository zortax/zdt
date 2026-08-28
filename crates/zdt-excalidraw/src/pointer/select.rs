//! Taking hold of what is already drawn.

use excalidraw::{Command, Id};
use kurbo::{Point, Rect};
use zgui::reactive::prelude::*;

use crate::state::{Board, Drag, Live};

use super::{Held, TOLERANCE};

/// What a press with the select tool starts.
pub(super) fn down(board: &Board, at: Point, scene_at: Point, held: Held) -> Option<Drag> {
    // A line's own points win over everything: they sit on top of it, and they are what the reader
    // is aiming at when a line is the only thing chosen.
    if let Some(drag) = point_drag(board, at) {
        return Some(drag);
    }

    // A handle wins over everything, because a handle sits over what it belongs to.
    if let Some(frame) = super::frame_untracked(board)
        && let Some(grip) = frame.grip(at)
    {
        return Some(match super::grip_drag(board, &frame, grip) {
            // A turn is about a point in the drawing, not a point on the screen: the frame knows
            // its middle in view pixels, and the command wants it where the elements are.
            Drag::Rotate { .. } => {
                let about = board.viewport.scene_point(frame.center());
                Drag::Rotate {
                    about,
                    start: angle(about, scene_at),
                }
            }
            other => other,
        });
    }

    let scene = board.read_untracked();
    let tolerance = TOLERANCE / board.viewport.zoom_untracked().max(f64::EPSILON);
    let hit = scene.hit(scene_at, tolerance).map(|held| held.id.clone());
    drop(scene);

    match hit {
        Some(id) => {
            let already = board.read_untracked().is_selected(&id);
            board.with_scene(|scene| {
                if held.adding {
                    if already {
                        // Holding the key and pressing again lets go of it.
                        let rest: Vec<Id> = scene
                            .selection()
                            .iter()
                            .filter(|held| **held != id)
                            .cloned()
                            .collect();
                        scene.select(rest);
                    } else {
                        scene.add_to_selection([id.clone()]);
                    }
                } else if !already {
                    scene.select([id.clone()]);
                }
            });
            // Letting go of something is not the start of a drag.
            board.read_untracked().has_selection().then_some(Drag::Move)
        }
        None => {
            // A press inside the frame but not on anything drags the whole selection.
            if let Some(frame) = super::frame_untracked(board)
                && frame.holds(at)
            {
                return Some(Drag::Move);
            }
            if !held.adding {
                board.with_scene(excalidraw::Scene::clear_selection);
            }
            Some(Drag::Band)
        }
    }
}

/// The point drag a press starts, when a line is chosen and the press is on one of its handles.
fn point_drag(board: &Board, at: Point) -> Option<Drag> {
    let scene = board.read_untracked();
    let chosen = scene.selection();
    if chosen.len() != 1 {
        return None;
    }
    let element = scene.element(chosen.first()?)?;
    if !element.kind.is_linear() || element.locked {
        return None;
    }
    let handles = crate::handles::point_handles(element, &board.viewport);
    let grip = crate::handles::grip_point(&handles, at)?;

    let placement = excalidraw::geom::Placement::of(element);
    let mut points: Vec<Point> = element
        .linear()?
        .points
        .iter()
        .map(|point| placement.scene(*point))
        .collect();
    if !grip.real {
        // The middle of a segment becomes a point of its own, and the drag moves it from there.
        points.insert(grip.index, board.viewport.scene_point(grip.at));
    }
    let id = element.id.clone();
    drop(scene);

    board.editing_points.set(Some(id.clone()));
    Some(Drag::Point {
        id,
        points,
        at: grip.index,
    })
}

/// Which way `at` lies from `about`.
fn angle(about: Point, at: Point) -> f64 {
    (at.y - about.y).atan2(at.x - about.x)
}

/// Selects everything the band wholly holds.
pub(super) fn band(board: &Board, live: &Live) {
    let box_ = Rect::from_points(live.from, live.at);
    let scene = board.read_untracked();
    let taken: Vec<Id> = excalidraw::hit::within(scene.elements(), box_)
        .into_iter()
        .filter_map(|at| scene.elements().get(at).map(|held| held.id.clone()))
        .collect();
    drop(scene);
    board.with_scene(|scene| {
        if live.constrained {
            scene.add_to_selection(taken);
        } else {
            scene.select(taken);
        }
    });
}

/// The change a finished move, scale or turn makes.
pub(super) fn commit(board: &Board, live: &Live) -> bool {
    commit_inner(board, live).unwrap_or(false)
}

/// The same, with the early exits a question mark gives.
fn commit_inner(board: &Board, live: &Live) -> Option<bool> {
    let ids: Vec<Id> = board.read_untracked().selection().to_vec();
    if ids.is_empty() {
        return None;
    }
    Some(match &live.drag {
        Drag::Move => board.apply(Command::Translate {
            ids,
            by: live.delta(),
        }),
        Drag::Resize { from, .. } => {
            // The same transform the drawing is being painted through, so what is written is what
            // was shown.
            let drag = super::transform_of(live)?;
            let to = drag.transform_rect_bbox(*from);
            board.apply(Command::Resize {
                ids,
                from: *from,
                to,
            })
        }
        Drag::Point { id, points, at } => {
            let mut points = points.clone();
            *points.get_mut(*at)? = live.at;
            // The drag holds the points where they are in the drawing; the command wants them in
            // the element's own space.
            let placement = excalidraw::geom::Placement::of(board.read_untracked().element(id)?);
            board.apply(Command::SetPoints {
                id: id.clone(),
                points: points.iter().map(|at| placement.local(*at)).collect(),
                pressures: Vec::new(),
            })
        }
        Drag::Rotate { about, start } => {
            let by = turned(live, *about, *start);
            board.apply(Command::Rotate {
                ids,
                angle: by,
                about: *about,
            })
        }
        _ => false,
    })
}

/// How far a turn has come, from where it started to where the pointer is.
///
/// Both angles are measured in the drawing, about the point the turn is about, so the answer does
/// not change with the zoom.
pub fn turned(live: &Live, about: Point, start: f64) -> f64 {
    let mut by = angle(about, live.at) - start;
    if live.constrained {
        // Fifteen degrees, which is what shift asks for.
        let step = std::f64::consts::PI / 12.0;
        by = (by / step).round() * step;
    }
    by
}
