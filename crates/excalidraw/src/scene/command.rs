//! Everything a drawing session does to a drawing.
//!
//! Each command finds the elements it names in the store, writes the keys it changes, and stamps
//! them as changed. Nothing else in the store is touched, which is what keeps a saved file a small
//! diff.

use kurbo::{Point, Rect, Vec2};
use rustc_hash::FxHashSet;
use serde_json::Value;

use crate::element::{Binding, Id, Kind};
use crate::geom;
use crate::store::Number;

use super::{Change, Order, Scene, build, order};

/// One change to a drawing. One command is one step of the undo history.
#[derive(Clone, PartialEq, Debug)]
pub enum Command {
    /// Puts new elements at the front of the order.
    Insert(Vec<Value>),
    /// Marks elements as removed. They stay in the file.
    Delete(Vec<Id>),
    /// Moves elements by `by`.
    Translate {
        /// Which elements.
        ids: Vec<Id>,
        /// How far.
        by: Vec2,
    },
    /// Scales elements from the box they are in onto another.
    Resize {
        /// Which elements.
        ids: Vec<Id>,
        /// The box they are in now.
        from: Rect,
        /// The box they go into.
        to: Rect,
    },
    /// Turns elements about `about`.
    Rotate {
        /// Which elements.
        ids: Vec<Id>,
        /// How far, in radians.
        angle: f64,
        /// What they turn about.
        about: Point,
    },
    /// Changes how elements look.
    Restyle {
        /// Which elements.
        ids: Vec<Id>,
        /// What changes.
        change: Change,
    },
    /// Moves elements through the painting order.
    Reorder {
        /// Which elements.
        ids: Vec<Id>,
        /// Which way.
        order: Order,
    },
    /// Puts elements in a group of their own.
    Group(Vec<Id>),
    /// Takes them out of their outermost one.
    Ungroup(Vec<Id>),
    /// Replaces the points of a line, an arrow or a pen stroke.
    SetPoints {
        /// Which element.
        id: Id,
        /// Where it goes now, in its own coordinates.
        points: Vec<Point>,
        /// How hard the pen was pressed, for a pen stroke.
        pressures: Vec<f64>,
    },
    /// Replaces the words of a text element, and the box they need.
    SetText {
        /// Which element.
        id: Id,
        /// What is drawn, with the breaks the wrapping put in.
        text: String,
        /// What was typed.
        original_text: String,
        /// How wide the words are.
        width: f64,
        /// How tall.
        height: f64,
    },
    /// Fixes an arrow's end to a shape, or lets it go.
    Bind {
        /// Which arrow.
        arrow: Id,
        /// Which end of it.
        start: bool,
        /// Where it is fixed, or nothing to let it go.
        to: Option<Binding>,
    },
    /// Puts elements into a frame, or takes them out of one.
    SetFrame {
        /// Which elements.
        ids: Vec<Id>,
        /// Which frame, or nothing to take them out.
        frame: Option<Id>,
    },
    /// Changes what is saved with the drawing.
    Settings(crate::Settings),
}

/// Does `command`, and answers whether anything changed.
pub(super) fn apply(scene: &mut Scene, command: Command) -> bool {
    // A change that moved a shape is not finished until the arrows fixed to it have followed.
    let moves_shapes = matches!(
        command,
        Command::Translate { .. } | Command::Resize { .. } | Command::Rotate { .. }
    );
    let touched: Vec<Id> = match &command {
        Command::Translate { ids, .. }
        | Command::Resize { ids, .. }
        | Command::Rotate { ids, .. } => ids.clone(),
        _ => Vec::new(),
    };

    let moved = one(scene, command);
    if moved && moves_shapes {
        scene.drawing.reread();
        follow_bindings(scene, &touched);
    }
    moved
}

/// Does one command.
fn one(scene: &mut Scene, command: Command) -> bool {
    match command {
        Command::Insert(elements) => insert(scene, elements),
        Command::Delete(ids) => delete(scene, &ids),
        Command::Translate { ids, by } => translate(scene, &ids, by),
        Command::Resize { ids, from, to } => resize(scene, &ids, from, to),
        Command::Rotate { ids, angle, about } => rotate(scene, &ids, angle, about),
        Command::Restyle { ids, change } => restyle(scene, &ids, &change),
        Command::Reorder { ids, order } => reorder(scene, &ids, order),
        Command::Group(ids) => group(scene, &ids),
        Command::Ungroup(ids) => ungroup(scene, &ids),
        Command::SetPoints {
            id,
            points,
            pressures,
        } => set_points(scene, &id, &points, &pressures),
        Command::SetText {
            id,
            text,
            original_text,
            width,
            height,
        } => set_text(scene, &id, &text, &original_text, width, height),
        Command::Bind { arrow, start, to } => bind(scene, &arrow, start, to.as_ref()),
        Command::SetFrame { ids, frame } => set_frame(scene, &ids, frame.as_ref()),
        Command::Settings(settings) => set_settings(scene, &settings),
    }
}

/// Moves the ends of every arrow fixed to one of `moved`.
///
/// An arrow that moved with the shape is left alone: it has already gone where it was going, and
/// moving its ends again would drag them twice.
fn follow_bindings(scene: &mut Scene, moved: &[Id]) {
    let moved: FxHashSet<&Id> = moved.iter().collect();

    // Which arrows are fixed to something that moved, and to what.
    let mut following: Vec<Id> = Vec::new();
    for element in &scene.drawing.elements {
        if moved.contains(&element.id) {
            continue;
        }
        let Some(linear) = element.linear() else {
            continue;
        };
        let bound = [&linear.start_binding, &linear.end_binding]
            .into_iter()
            .flatten()
            .any(|binding| moved.contains(&binding.element));
        if bound {
            following.push(element.id.clone());
        }
    }

    for id in following {
        let Some((_, arrow)) = scene.drawing.find(&id) else {
            continue;
        };
        let arrow = arrow.clone();
        let Some(linear) = arrow.linear() else {
            continue;
        };
        let shape_of = |binding: Option<&crate::element::Binding>| {
            binding.and_then(|binding| {
                scene
                    .drawing
                    .find(&binding.element)
                    .map(|(_, held)| held.clone())
            })
        };
        let start = shape_of(linear.start_binding.as_ref());
        let end = shape_of(linear.end_binding.as_ref());
        let Some(points) = geom::binding::moved_points(&arrow, start.as_ref(), end.as_ref()) else {
            continue;
        };
        let origin = geom::binding::moved_origin(&arrow, start.as_ref())
            .unwrap_or_else(|| Point::new(arrow.x, arrow.y));
        // The points come back in the arrow's own space; `set_points` wants them in the scene's.
        let scene_points: Vec<Point> = points
            .into_iter()
            .map(|point| Point::new(origin.x + point.x, origin.y + point.y))
            .collect();
        set_points(scene, &id, &scene_points, &[]);
        scene.drawing.reread();
    }
}

/// Where in the store each of `ids` sits.
fn places(scene: &Scene, ids: &[Id]) -> Vec<usize> {
    let wanted: FxHashSet<&str> = ids.iter().map(Id::as_str).collect();
    scene
        .drawing
        .store
        .elements()
        .iter()
        .enumerate()
        .filter(|(_, held)| {
            held.get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| wanted.contains(id))
        })
        .map(|(at, _)| at)
        .collect()
}

/// Marks the element at `at` as changed, so a peer reconciling two drawings can tell.
fn touch(scene: &mut Scene, at: usize) {
    // An element with no version has been changed once already, as the reader counts it.
    let version = scene
        .drawing
        .store
        .element(at)
        .and_then(|held| held.get("version"))
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .max(1);
    let nonce = scene.fresh_nonce();
    let now = scene.now();
    scene
        .drawing
        .store
        .patch(at, "version", Value::from(version + 1));
    scene
        .drawing
        .store
        .patch(at, "versionNonce", Value::from(nonce));
    scene.drawing.store.patch(at, "updated", Value::from(now));
}

/// Writes `values` on the element at `at`, and marks it changed when anything moved.
fn write(scene: &mut Scene, at: usize, values: Vec<(String, Value)>) -> bool {
    if !scene.drawing.store.patch_all(at, values) {
        return false;
    }
    touch(scene, at);
    true
}

/// Puts new elements at the front of the order.
fn insert(scene: &mut Scene, elements: Vec<Value>) -> bool {
    if elements.is_empty() {
        return false;
    }
    for element in elements {
        scene.drawing.store.push(element);
    }
    resync_indices(scene);
    true
}

/// Marks elements as removed.
fn delete(scene: &mut Scene, ids: &[Id]) -> bool {
    let mut moved = false;
    for at in places(scene, ids) {
        moved |= write(scene, at, vec![("isDeleted".to_owned(), Value::Bool(true))]);
    }
    moved
}

/// Moves elements by `by`.
///
/// A frame carries what is in it: moving the frame and leaving its children behind would be moving
/// the outline rather than the thing.
fn translate(scene: &mut Scene, ids: &[Id], by: Vec2) -> bool {
    if by.hypot() < f64::EPSILON {
        return false;
    }
    let ids = with_carried(scene, ids);
    let mut moved = false;
    for at in places(scene, &ids) {
        let Some(held) = scene.drawing.store.element(at) else {
            continue;
        };
        let x = crate::element::number(held, "x").unwrap_or(0.0) + by.x;
        let y = crate::element::number(held, "y").unwrap_or(0.0) + by.y;
        moved |= write(
            scene,
            at,
            vec![
                ("x".to_owned(), Number::json(x)),
                ("y".to_owned(), Number::json(y)),
            ],
        );
    }
    moved
}

/// These ids, and everything they carry: what is in a frame, and the words written in a shape.
fn with_carried(scene: &Scene, ids: &[Id]) -> Vec<Id> {
    let chosen: FxHashSet<&Id> = ids.iter().collect();
    let frames: FxHashSet<&Id> = ids
        .iter()
        .filter(|id| {
            scene
                .element(id)
                .is_some_and(|held| matches!(held.kind, Kind::Frame | Kind::Magicframe))
        })
        .collect();

    let mut out = ids.to_vec();
    for element in &scene.drawing.elements {
        let in_frame = element
            .frame_id
            .as_ref()
            .is_some_and(|frame| frames.contains(frame));
        // Words written inside a shape go where the shape goes.
        let in_shape = element
            .text()
            .and_then(|words| words.container_id.as_ref())
            .is_some_and(|container| chosen.contains(container));
        if (in_frame || in_shape) && !out.contains(&element.id) {
            out.push(element.id.clone());
        }
    }
    out
}

/// Scales elements from one box onto another.
fn resize(scene: &mut Scene, ids: &[Id], from: Rect, to: Rect) -> bool {
    if from.width() <= 0.0 || from.height() <= 0.0 {
        return false;
    }
    let scale_x = to.width() / from.width();
    let scale_y = to.height() / from.height();
    if (scale_x - 1.0).abs() < 1e-12
        && (scale_y - 1.0).abs() < 1e-12
        && (to.x0 - from.x0).abs() < 1e-12
        && (to.y0 - from.y0).abs() < 1e-12
    {
        return false;
    }

    let mut moved = false;
    for at in places(scene, ids) {
        let Some(held) = scene.drawing.store.element(at) else {
            continue;
        };
        let x = crate::element::number(held, "x").unwrap_or(0.0);
        let y = crate::element::number(held, "y").unwrap_or(0.0);
        let width = crate::element::number(held, "width").unwrap_or(0.0);
        let height = crate::element::number(held, "height").unwrap_or(0.0);

        let mut values = vec![
            (
                "x".to_owned(),
                Number::json(to.x0 + (x - from.x0) * scale_x),
            ),
            (
                "y".to_owned(),
                Number::json(to.y0 + (y - from.y0) * scale_y),
            ),
            ("width".to_owned(), Number::json(width * scale_x)),
            ("height".to_owned(), Number::json(height * scale_y)),
        ];
        // A line and a pen stroke are their points, so those are scaled too.
        if let Some(points) = held.get("points").and_then(Value::as_array) {
            let scaled: Vec<Point> = points
                .iter()
                .filter_map(|point| {
                    let pair = point.as_array()?;
                    Some(Point::new(
                        pair.first()?.as_f64()? * scale_x,
                        pair.get(1)?.as_f64()? * scale_y,
                    ))
                })
                .collect();
            values.push(("points".to_owned(), build::points_json(&scaled)));
        }
        // Words scale with their box, so a resized label reads at the size it was dragged to.
        if let Some(size) = crate::element::number(held, "fontSize") {
            let by = ((scale_x.abs() + scale_y.abs()) / 2.0).max(0.01);
            values.push(("fontSize".to_owned(), Number::json(size * by)));
        }
        moved |= write(scene, at, values);
    }
    moved
}

/// Turns elements about `about`.
fn rotate(scene: &mut Scene, ids: &[Id], angle: f64, about: Point) -> bool {
    if angle.abs() < 1e-12 {
        return false;
    }
    let mut moved = false;
    for at in places(scene, ids) {
        let Some(held) = scene.drawing.store.element(at) else {
            continue;
        };
        let x = crate::element::number(held, "x").unwrap_or(0.0);
        let y = crate::element::number(held, "y").unwrap_or(0.0);
        let width = crate::element::number(held, "width").unwrap_or(0.0);
        let height = crate::element::number(held, "height").unwrap_or(0.0);
        let was = crate::element::number(held, "angle").unwrap_or(0.0);

        // The element turns about its own middle, and its middle turns about `about`.
        let center = Point::new(x + width / 2.0, y + height / 2.0);
        let moved_center = geom::rotated(center, about, angle);
        moved |= write(
            scene,
            at,
            vec![
                ("x".to_owned(), Number::json(moved_center.x - width / 2.0)),
                ("y".to_owned(), Number::json(moved_center.y - height / 2.0)),
                ("angle".to_owned(), Number::json(normalized(was + angle))),
            ],
        );
    }
    moved
}

/// An angle brought back inside one turn.
fn normalized(angle: f64) -> f64 {
    let turn = std::f64::consts::TAU;
    let held = angle % turn;
    if held < 0.0 { held + turn } else { held }
}

/// Changes how elements look.
fn restyle(scene: &mut Scene, ids: &[Id], change: &Change) -> bool {
    let mut moved = false;
    for at in places(scene, ids) {
        let Some(held) = scene.drawing.store.element(at) else {
            continue;
        };
        let Some(kind) = held
            .get("type")
            .and_then(Value::as_str)
            .and_then(|word| serde_json::from_value::<Kind>(Value::String(word.to_owned())).ok())
        else {
            continue;
        };
        if !change.applies_to(kind) {
            continue;
        }
        let values = match change {
            Change::StrokeColor(color) => {
                vec![("strokeColor".to_owned(), Value::String(color.clone()))]
            }
            Change::BackgroundColor(color) => {
                vec![("backgroundColor".to_owned(), Value::String(color.clone()))]
            }
            Change::FillStyle(style) => vec![("fillStyle".to_owned(), word(*style))],
            Change::StrokeWidth(width) => {
                vec![("strokeWidth".to_owned(), Number::json(*width))]
            }
            Change::StrokeStyle(style) => vec![("strokeStyle".to_owned(), word(*style))],
            Change::Roughness(roughness) => {
                vec![("roughness".to_owned(), Number::json(*roughness))]
            }
            Change::Opacity(opacity) => vec![(
                "opacity".to_owned(),
                Number::json(opacity.clamp(0.0, 100.0)),
            )],
            Change::Roundness(roundness) => vec![(
                "roundness".to_owned(),
                build::roundness_value(kind, *roundness),
            )],
            Change::FontSize(size) => vec![("fontSize".to_owned(), Number::json(*size))],
            Change::FontFamily(family) => {
                vec![("fontFamily".to_owned(), Value::from(family.to_number()))]
            }
            Change::TextAlign(align) => vec![("textAlign".to_owned(), word(*align))],
            Change::VerticalAlign(align) => vec![("verticalAlign".to_owned(), word(*align))],
            Change::StartArrowhead(head) => vec![("startArrowhead".to_owned(), head_word(*head))],
            Change::EndArrowhead(head) => vec![("endArrowhead".to_owned(), head_word(*head))],
            Change::Locked(locked) => vec![("locked".to_owned(), Value::Bool(*locked))],
            Change::Link(link) => vec![(
                "link".to_owned(),
                link.clone().map_or(Value::Null, Value::String),
            )],
        };
        moved |= write(scene, at, values);
    }
    moved
}

/// A word, as the file writes it.
fn word<T: serde::Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// A head, as the file writes it.
fn head_word(head: Option<crate::element::Arrowhead>) -> Value {
    head.map_or(Value::Null, |head| Value::String(head.as_str().to_owned()))
}

/// Moves elements through the painting order.
fn reorder(scene: &mut Scene, ids: &[Id], which: Order) -> bool {
    let moving = places(scene, ids);
    if moving.is_empty() {
        return false;
    }
    let count = scene.drawing.store.len();
    let order = order::reordered(count, &moving, which);
    if order.iter().enumerate().all(|(to, from)| to == *from) {
        return false;
    }
    scene.drawing.store.reorder(&order);
    resync_indices(scene);
    true
}

/// Puts elements in a group of their own.
fn group(scene: &mut Scene, ids: &[Id]) -> bool {
    let at = places(scene, ids);
    if at.len() < 2 {
        return false;
    }
    let group = scene.fresh_id();
    let mut moved = false;
    for at in at {
        let Some(held) = scene.drawing.store.element(at) else {
            continue;
        };
        let mut groups: Vec<Value> = held
            .get("groupIds")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        // The new group is the outermost one, so it is what a click takes hold of.
        groups.push(Value::String(group.as_str().to_owned()));
        moved |= write(
            scene,
            at,
            vec![("groupIds".to_owned(), Value::Array(groups))],
        );
    }
    moved
}

/// Takes elements out of their outermost group.
fn ungroup(scene: &mut Scene, ids: &[Id]) -> bool {
    let mut moved = false;
    for at in places(scene, ids) {
        let Some(held) = scene.drawing.store.element(at) else {
            continue;
        };
        let mut groups: Vec<Value> = held
            .get("groupIds")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if groups.pop().is_none() {
            continue;
        }
        moved |= write(
            scene,
            at,
            vec![("groupIds".to_owned(), Value::Array(groups))],
        );
    }
    moved
}

/// Replaces the points of a line, an arrow or a pen stroke, and the box they need.
fn set_points(scene: &mut Scene, id: &Id, points: &[Point], pressures: &[f64]) -> bool {
    let Some(at) = places(scene, std::slice::from_ref(id)).first().copied() else {
        return false;
    };
    if points.is_empty() {
        return false;
    }
    // The first point is the element's origin, so whatever it is now is folded into x and y.
    let first = points[0];
    let local: Vec<Point> = points
        .iter()
        .map(|point| *point - first.to_vec2())
        .collect();
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
    for point in &local {
        min_x = min_x.min(point.x);
        max_x = max_x.max(point.x);
        min_y = min_y.min(point.y);
        max_y = max_y.max(point.y);
    }

    let Some(held) = scene.drawing.store.element(at) else {
        return false;
    };
    let x = crate::element::number(held, "x").unwrap_or(0.0) + first.x;
    let y = crate::element::number(held, "y").unwrap_or(0.0) + first.y;

    let mut values = vec![
        ("points".to_owned(), build::points_json(&local)),
        ("x".to_owned(), Number::json(x)),
        ("y".to_owned(), Number::json(y)),
        ("width".to_owned(), Number::json(max_x - min_x)),
        ("height".to_owned(), Number::json(max_y - min_y)),
    ];
    if !pressures.is_empty() {
        values.push((
            "pressures".to_owned(),
            Value::Array(pressures.iter().map(|held| Number::json(*held)).collect()),
        ));
    }
    write(scene, at, values)
}

/// Replaces the words of a text element.
fn set_text(
    scene: &mut Scene,
    id: &Id,
    text: &str,
    original_text: &str,
    width: f64,
    height: f64,
) -> bool {
    let Some(at) = places(scene, std::slice::from_ref(id)).first().copied() else {
        return false;
    };
    write(
        scene,
        at,
        vec![
            ("text".to_owned(), Value::String(text.to_owned())),
            (
                "originalText".to_owned(),
                Value::String(original_text.to_owned()),
            ),
            ("width".to_owned(), Number::json(width)),
            ("height".to_owned(), Number::json(height)),
        ],
    )
}

/// Fixes an arrow's end to a shape, or lets it go.
fn bind(scene: &mut Scene, arrow: &Id, start: bool, to: Option<&Binding>) -> bool {
    let Some(at) = places(scene, std::slice::from_ref(arrow)).first().copied() else {
        return false;
    };
    let key = if start { "startBinding" } else { "endBinding" };
    let value = to.map_or(Value::Null, |binding| {
        let mut object = serde_json::Map::new();
        object.insert(
            "elementId".to_owned(),
            Value::String(binding.element.as_str().to_owned()),
        );
        object.insert(
            "fixedPoint".to_owned(),
            Value::Array(vec![
                Number::json(binding.fixed_point.0),
                Number::json(binding.fixed_point.1),
            ]),
        );
        object.insert(
            "mode".to_owned(),
            Value::String(
                match binding.mode {
                    crate::element::BindMode::Inside => "inside",
                    crate::element::BindMode::Orbit => "orbit",
                    crate::element::BindMode::Skip => "skip",
                }
                .to_owned(),
            ),
        );
        Value::Object(object)
    });

    let mut moved = write(scene, at, vec![(key.to_owned(), value)]);

    // The shape keeps a list of what is fixed to it, so moving the shape can find the arrows.
    if let Some(binding) = to {
        moved |= note_bound(scene, &binding.element, arrow);
    }
    moved
}

/// Notes on `shape` that `arrow` is fixed to it.
fn note_bound(scene: &mut Scene, shape: &Id, arrow: &Id) -> bool {
    let Some(at) = places(scene, std::slice::from_ref(shape)).first().copied() else {
        return false;
    };
    let Some(held) = scene.drawing.store.element(at) else {
        return false;
    };
    let mut bound: Vec<Value> = held
        .get("boundElements")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let already = bound
        .iter()
        .any(|held| held.get("id").and_then(Value::as_str) == Some(arrow.as_str()));
    if already {
        return false;
    }
    let mut entry = serde_json::Map::new();
    entry.insert("id".to_owned(), Value::String(arrow.as_str().to_owned()));
    entry.insert("type".to_owned(), Value::String("arrow".to_owned()));
    bound.push(Value::Object(entry));
    write(
        scene,
        at,
        vec![("boundElements".to_owned(), Value::Array(bound))],
    )
}

/// Puts elements into a frame, or takes them out of one.
fn set_frame(scene: &mut Scene, ids: &[Id], frame: Option<&Id>) -> bool {
    let value = frame.map_or(Value::Null, |id| Value::String(id.as_str().to_owned()));
    let mut moved = false;
    for at in places(scene, ids) {
        moved |= write(scene, at, vec![("frameId".to_owned(), value.clone())]);
    }
    moved
}

/// Changes what is saved with the drawing.
fn set_settings(scene: &mut Scene, settings: &crate::Settings) -> bool {
    if scene.drawing.settings == *settings {
        return false;
    }
    let Some(document) = scene.drawing.store.document_mut().as_object_mut() else {
        return false;
    };
    document.insert("appState".to_owned(), settings.to_json());
    true
}

/// Makes the order keys agree with the order again.
///
/// Only the keys that no longer sort with their neighbours are written, so an element that did not
/// move keeps the key it had.
fn resync_indices(scene: &mut Scene) {
    let keys: Vec<Option<String>> = scene
        .drawing
        .store
        .elements()
        .iter()
        .map(|held| held.get("index").and_then(Value::as_str).map(str::to_owned))
        .collect();
    let Ok(made) = crate::index::sync_invalid(&keys) else {
        return;
    };
    for (at, key) in made.into_iter().enumerate() {
        if keys.get(at).and_then(Clone::clone).as_deref() != Some(key.as_str()) {
            scene.drawing.store.patch(at, "index", Value::String(key));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Style;
    use crate::scene::tests::scene;

    #[test]
    fn moving_something_writes_only_where_it_is() {
        let mut scene =
            scene(r#"[{"type":"rectangle","id":"a","x":0,"y":0,"width":10,"height":10}]"#);
        assert!(scene.apply(Command::Translate {
            ids: vec![Id::new("a")],
            by: Vec2::new(5.0, 7.0),
        }));
        let held = scene.element(&Id::new("a")).expect("the element");
        assert!((held.x - 5.0).abs() < f64::EPSILON);
        assert!((held.y - 7.0).abs() < f64::EPSILON);
        assert_eq!(held.version, 2, "it was marked changed once");
    }

    #[test]
    fn a_move_of_nothing_writes_nothing() {
        let mut scene = scene(r#"[{"type":"rectangle","id":"a","x":0,"y":0}]"#);
        assert!(!scene.apply(Command::Translate {
            ids: vec![Id::new("a")],
            by: Vec2::ZERO,
        }));
        assert_eq!(scene.element(&Id::new("a")).expect("it").version, 1);
    }

    #[test]
    fn deleting_leaves_the_element_in_the_file() {
        let mut scene = scene(r#"[{"type":"rectangle","id":"a"}]"#);
        assert!(scene.apply(Command::Delete(vec![Id::new("a")])));
        assert_eq!(scene.elements().len(), 1);
        assert!(scene.element(&Id::new("a")).expect("it").is_deleted);
    }

    #[test]
    fn resizing_scales_the_box_and_the_points_inside_it() {
        let mut scene =
            scene(r#"[{"type":"line","id":"a","x":0,"y":0,"points":[[0,0],[10,0],[10,10]]}]"#);
        assert!(scene.apply(Command::Resize {
            ids: vec![Id::new("a")],
            from: Rect::new(0.0, 0.0, 10.0, 10.0),
            to: Rect::new(0.0, 0.0, 20.0, 10.0),
        }));
        let held = scene.element(&Id::new("a")).expect("it");
        let points = &held.linear().expect("a line").points;
        assert!((points[1].x - 20.0).abs() < 1e-9);
        assert!((points[2].y - 10.0).abs() < 1e-9);
    }

    #[test]
    fn turning_something_moves_it_round_the_point_it_turns_about() {
        let mut scene =
            scene(r#"[{"type":"rectangle","id":"a","x":100,"y":0,"width":10,"height":10}]"#);
        assert!(scene.apply(Command::Rotate {
            ids: vec![Id::new("a")],
            angle: std::f64::consts::FRAC_PI_2,
            about: Point::ZERO,
        }));
        let held = scene.element(&Id::new("a")).expect("it");
        // Its middle was at (105, 5); a quarter turn about the origin takes that to (-5, 105),
        // and the box hangs off the middle as it did before.
        assert!((held.x + 10.0).abs() < 1e-6, "x is {}", held.x);
        assert!((held.y - 100.0).abs() < 1e-6, "y is {}", held.y);
        assert!((held.angle - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    }

    #[test]
    fn a_restyling_only_reaches_what_it_means_anything_to() {
        let mut scene =
            scene(r#"[{"type":"rectangle","id":"a"},{"type":"text","id":"b","text":"hi"}]"#);
        assert!(scene.apply(Command::Restyle {
            ids: vec![Id::new("a"), Id::new("b")],
            change: Change::FontSize(30.0),
        }));
        assert_eq!(scene.element(&Id::new("a")).expect("it").version, 1);
        assert_eq!(scene.element(&Id::new("b")).expect("it").version, 2);
    }

    #[test]
    fn grouping_needs_two_and_puts_the_new_group_outermost() {
        let mut scene = scene(
            r#"[{"type":"rectangle","id":"a","groupIds":["inner"]},
                {"type":"rectangle","id":"b","groupIds":["inner"]}]"#,
        );
        assert!(!scene.apply(Command::Group(vec![Id::new("a")])));
        assert!(scene.apply(Command::Group(vec![Id::new("a"), Id::new("b")])));
        let held = scene.element(&Id::new("a")).expect("it");
        assert_eq!(held.group_ids.len(), 2);
        assert_eq!(held.group_ids[0], "inner");

        assert!(scene.apply(Command::Ungroup(vec![Id::new("a"), Id::new("b")])));
        assert_eq!(
            scene.element(&Id::new("a")).expect("it").group_ids,
            ["inner"]
        );
    }

    #[test]
    fn reordering_moves_the_element_and_gives_it_a_key_that_sorts() {
        let mut scene = scene(
            r#"[{"type":"rectangle","id":"a","index":"a0"},
                {"type":"rectangle","id":"b","index":"a1"},
                {"type":"rectangle","id":"c","index":"a2"}]"#,
        );
        assert!(scene.apply(Command::Reorder {
            ids: vec![Id::new("a")],
            order: Order::Front,
        }));
        let ids: Vec<&str> = scene
            .elements()
            .iter()
            .map(|held| held.id.as_str())
            .collect();
        assert_eq!(ids, ["b", "c", "a"]);
        let keys: Vec<&str> = scene
            .elements()
            .iter()
            .filter_map(|held| held.index.as_deref())
            .collect();
        for pair in keys.windows(2) {
            assert!(pair[0] < pair[1], "{} is not before {}", pair[0], pair[1]);
        }
    }

    #[test]
    fn something_already_at_the_front_does_not_move() {
        let mut scene = scene(r#"[{"type":"rectangle","id":"a","index":"a0"}]"#);
        assert!(!scene.apply(Command::Reorder {
            ids: vec![Id::new("a")],
            order: Order::Front,
        }));
    }

    #[test]
    fn a_new_element_lands_at_the_front_with_a_key_of_its_own() {
        let mut scene = scene(r#"[{"type":"rectangle","id":"a","index":"a0"}]"#);
        let made = build::element(
            Kind::Ellipse,
            build::Box_ {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            &Style::default(),
            &Id::new("new"),
            crate::element::Seed(7),
            9,
            1,
        );
        assert!(scene.apply(Command::Insert(vec![made])));
        assert_eq!(scene.elements().len(), 2);
        let last = scene.elements().last().expect("it");
        assert_eq!(last.id.as_str(), "new");
        assert!(last.index.as_deref().expect("a key") > "a0");
    }

    #[test]
    fn binding_an_arrow_notes_it_on_the_shape_as_well() {
        let mut scene = scene(
            r#"[{"type":"rectangle","id":"box","width":50,"height":50},
                {"type":"arrow","id":"arr","points":[[0,0],[10,0]]}]"#,
        );
        assert!(scene.apply(Command::Bind {
            arrow: Id::new("arr"),
            start: true,
            to: Some(Binding {
                element: Id::new("box"),
                fixed_point: (1.0, 0.5001),
                mode: crate::element::BindMode::Orbit,
            }),
        }));
        let arrow = scene.element(&Id::new("arr")).expect("it");
        assert!(arrow.linear().expect("an arrow").start_binding.is_some());
        let shape = scene.element(&Id::new("box")).expect("it");
        assert_eq!(shape.bound_elements.len(), 1);
        assert_eq!(shape.bound_elements[0].id.as_str(), "arr");
    }

    #[test]
    fn setting_the_points_moves_the_origin_with_them() {
        let mut scene = scene(r#"[{"type":"line","id":"a","x":10,"y":10,"points":[[0,0],[5,0]]}]"#);
        assert!(scene.apply(Command::SetPoints {
            id: Id::new("a"),
            points: vec![Point::new(-5.0, 0.0), Point::new(15.0, 0.0)],
            pressures: Vec::new(),
        }));
        let held = scene.element(&Id::new("a")).expect("it");
        assert!(
            (held.x - 5.0).abs() < 1e-9,
            "the origin followed the first point"
        );
        assert_eq!(held.linear().expect("a line").points[0], Point::ZERO);
        assert!((held.width - 20.0).abs() < 1e-9);
    }

    #[test]
    fn setting_the_words_writes_what_was_typed_as_well() {
        let mut scene = scene(r#"[{"type":"text","id":"a","text":"hi","originalText":"hi"}]"#);
        assert!(scene.apply(Command::SetText {
            id: Id::new("a"),
            text: "hello\nthere".to_owned(),
            original_text: "hello there".to_owned(),
            width: 80.0,
            height: 50.0,
        }));
        let words = scene
            .element(&Id::new("a"))
            .expect("it")
            .text()
            .expect("words")
            .clone();
        assert_eq!(words.text, "hello\nthere");
        assert_eq!(words.original_text, "hello there");
    }

    #[test]
    fn moving_a_frame_carries_what_is_in_it() {
        let mut scene = scene(
            r#"[{"type":"frame","id":"f","x":0,"y":0,"width":200,"height":200},
                {"type":"rectangle","id":"in","x":20,"y":20,"width":10,"height":10,
                 "frameId":"f"},
                {"type":"rectangle","id":"out","x":300,"y":20,"width":10,"height":10}]"#,
        );
        assert!(scene.apply(Command::Translate {
            ids: vec![Id::new("f")],
            by: Vec2::new(50.0, 0.0),
        }));
        assert!((scene.element(&Id::new("in")).expect("it").x - 70.0).abs() < 1e-9);
        assert!(
            (scene.element(&Id::new("out")).expect("it").x - 300.0).abs() < 1e-9,
            "what is not in it stays where it is"
        );
    }

    #[test]
    fn moving_a_shape_carries_the_words_written_in_it() {
        let mut scene = scene(
            r#"[{"type":"rectangle","id":"box","x":0,"y":0,"width":200,"height":100,
                 "boundElements":[{"id":"t","type":"text"}]},
                {"type":"text","id":"t","x":20,"y":40,"width":160,"height":25,"text":"in",
                 "containerId":"box"}]"#,
        );
        assert!(scene.apply(Command::Translate {
            ids: vec![Id::new("box")],
            by: Vec2::new(50.0, 10.0),
        }));
        let label = scene.element(&Id::new("t")).expect("the words");
        assert!((label.x - 70.0).abs() < 1e-9, "x is {}", label.x);
        assert!((label.y - 50.0).abs() < 1e-9, "y is {}", label.y);
    }

    #[test]
    fn moving_a_shape_drags_the_arrow_bound_to_it() {
        let mut scene = scene(
            r#"[{"type":"rectangle","id":"box","x":200,"y":0,"width":100,"height":100,
                 "boundElements":[{"id":"arr","type":"arrow"}]},
                {"type":"arrow","id":"arr","x":0,"y":50,"points":[[0,0],[100,0]],
                 "endBinding":{"elementId":"box","fixedPoint":[0,0.5001],"mode":"inside"}}]"#,
        );
        let before = scene.element(&Id::new("arr")).expect("the arrow").width;

        assert!(scene.apply(Command::Translate {
            ids: vec![Id::new("box")],
            by: Vec2::new(100.0, 0.0),
        }));

        let after = scene.element(&Id::new("arr")).expect("the arrow").width;
        assert!(
            after > before + 90.0,
            "the arrow stretched to follow: {before} to {after}"
        );
    }

    #[test]
    fn an_arrow_that_moved_with_its_shape_is_not_dragged_twice() {
        let mut scene = scene(
            r#"[{"type":"rectangle","id":"box","x":200,"y":0,"width":100,"height":100,
                 "boundElements":[{"id":"arr","type":"arrow"}]},
                {"type":"arrow","id":"arr","x":0,"y":50,"points":[[0,0],[100,0]],
                 "endBinding":{"elementId":"box","fixedPoint":[0,0.5001],"mode":"inside"}}]"#,
        );
        assert!(scene.apply(Command::Translate {
            ids: vec![Id::new("box"), Id::new("arr")],
            by: Vec2::new(100.0, 0.0),
        }));
        let arrow = scene.element(&Id::new("arr")).expect("the arrow");
        assert!((arrow.x - 100.0).abs() < 1e-9, "x is {}", arrow.x);
        assert!((arrow.width - 100.0).abs() < 0.1, "it kept its length");
    }

    #[test]
    fn a_command_that_changes_nothing_leaves_the_file_untouched() {
        let text = r#"{"type":"excalidraw","version":2,"elements":[
  {
    "type": "rectangle",
    "id": "a",
    "x": 0,
    "y": 0
  }
]}"#;
        let drawing = crate::file::parse(text).expect("a drawing");
        let before = drawing.to_string().expect("it writes");
        let mut scene = Scene::new(drawing, 1, 1);
        assert!(!scene.apply(Command::Translate {
            ids: vec![Id::new("a")],
            by: Vec2::ZERO,
        }));
        assert_eq!(scene.to_string().expect("it writes"), before);
    }
}
