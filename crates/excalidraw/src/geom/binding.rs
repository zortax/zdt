//! Where a bound arrow's end sits.
//!
//! An arrow bound to a shape names a point on that shape as a fraction of its width and height. So
//! the shape can be moved, scaled or turned and the arrow's end still knows where to be: the
//! fraction is read against the shape as it is now.

use kurbo::{Point, Vec2};

use crate::element::{BindMode, Binding, Element};

use super::{Placement, unit};

/// How far outside a shape an orbiting arrow is held off, before the shape's own outline width.
pub const BASE_GAP: f64 = 5.0;
/// The smallest a shape may be and still be measured against.
const SMALLEST: f64 = 1.0;

/// How far off `shape` an arrow bound to it is held.
#[must_use]
pub fn gap(shape: &Element) -> f64 {
    BASE_GAP + shape.stroke_width / 2.0
}

/// Where on `shape` the binding points, in the scene.
#[must_use]
pub fn point(binding: &Binding, shape: &Element) -> Point {
    let placement = Placement::of(shape);
    placement.scene(Point::new(
        shape.width * binding.fixed_point.0,
        shape.height * binding.fixed_point.1,
    ))
}

/// Where a bound end sits, held off the shape when the binding asks for it.
///
/// `toward` is the arrow's next point, which is the direction the end is held off along.
#[must_use]
pub fn end(binding: &Binding, shape: &Element, toward: Point) -> Point {
    let at = point(binding, shape);
    match binding.mode {
        // Inside means exactly where the fraction says, and skipping means the arrow is not moved.
        BindMode::Inside | BindMode::Skip => at,
        BindMode::Orbit => {
            let along = unit(toward - at);
            if along.hypot() < f64::EPSILON {
                at
            } else {
                at + along * gap(shape)
            }
        }
    }
}

/// The binding an arrow ending at `at` would take to `shape`.
///
/// Answers nothing when the shape cannot be bound to. The fraction is measured against the shape as
/// it stands, so the same point on a shape that is later scaled stays the same point on it.
#[must_use]
pub fn to(shape: &Element, at: Point) -> Option<Binding> {
    if !shape.kind.is_bindable() || shape.is_deleted {
        return None;
    }
    let placement = Placement::of(shape);
    let local = placement.local(at);
    let width = shape.width.max(SMALLEST);
    let height = shape.height.max(SMALLEST);
    let inside =
        local.x >= 0.0 && local.x <= shape.width && local.y >= 0.0 && local.y <= shape.height;
    Some(Binding {
        element: shape.id.clone(),
        fixed_point: normalized((local.x / width, local.y / height)),
        mode: if inside {
            BindMode::Inside
        } else {
            BindMode::Orbit
        },
    })
}

/// A fraction kept inside the range a shape can be measured over.
///
/// Exactly half is nudged off: a point on the middle line flips which side of the shape the arrow
/// leaves from every time the shape is nudged.
fn normalized((x, y): (f64, f64)) -> (f64, f64) {
    let one = |value: f64| {
        let value = if value.is_finite() {
            value.clamp(-10.0, 10.0)
        } else {
            0.5001
        };
        if (value - 0.5).abs() < 1e-4 {
            0.5001
        } else {
            value
        }
    };
    (one(x), one(y))
}

/// The points `arrow` would have with its bound ends moved to where its shapes are now.
///
/// Answers nothing when neither end is bound, or when nothing would move. The points come back in
/// the arrow's own coordinates, with the first still at the origin.
#[must_use]
pub fn moved_points(
    arrow: &Element,
    start: Option<&Element>,
    end_shape: Option<&Element>,
) -> Option<Vec<Point>> {
    let linear = arrow.linear()?;
    if linear.points.len() < 2 {
        return None;
    }
    let placement = Placement::of(arrow);
    let mut points: Vec<Point> = linear
        .points
        .iter()
        .map(|point| placement.scene(*point))
        .collect();
    let count = points.len();
    let mut moved = false;

    if let (Some(binding), Some(shape)) = (linear.start_binding.as_ref(), start) {
        let held = end(binding, shape, points[1]);
        if (held - points[0]).hypot() > 1e-9 {
            points[0] = held;
            moved = true;
        }
    }
    if let (Some(binding), Some(shape)) = (linear.end_binding.as_ref(), end_shape) {
        let held = end(binding, shape, points[count - 2]);
        if (held - points[count - 1]).hypot() > 1e-9 {
            points[count - 1] = held;
            moved = true;
        }
    }
    if !moved {
        return None;
    }
    // Back to the arrow's own space, with the first point at its origin.
    let origin = points[0];
    Some(
        points
            .into_iter()
            .map(|point| point - Vec2::new(origin.x, origin.y))
            .map(|held| Point::new(held.x, held.y))
            .collect(),
    )
}

/// Where the arrow's origin ends up once its bound ends have moved.
#[must_use]
pub fn moved_origin(arrow: &Element, start: Option<&Element>) -> Option<Point> {
    let linear = arrow.linear()?;
    if linear.points.len() < 2 {
        return None;
    }
    let placement = Placement::of(arrow);
    let binding = linear.start_binding.as_ref()?;
    let shape = start?;
    let toward = placement.scene(linear.points[1]);
    Some(end(binding, shape, toward))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(json: &str) -> Element {
        let value: serde_json::Value = serde_json::from_str(json).expect("valid JSON");
        crate::element::read(value.as_object().expect("an object")).expect("an element")
    }

    #[test]
    fn a_binding_points_at_the_same_place_on_a_moved_shape() {
        let here = read(r#"{"type":"rectangle","id":"a","x":0,"y":0,"width":100,"height":100}"#);
        let binding = to(&here, Point::new(100.0, 50.0)).expect("a binding");
        assert!((binding.fixed_point.0 - 1.0).abs() < 1e-9);

        let there = read(r#"{"type":"rectangle","id":"a","x":200,"y":0,"width":100,"height":100}"#);
        let at = point(&binding, &there);
        assert!((at.x - 300.0).abs() < 1e-9, "it moved with the shape");
    }

    #[test]
    fn a_binding_points_at_the_same_place_on_a_scaled_shape() {
        let small = read(r#"{"type":"rectangle","id":"a","x":0,"y":0,"width":100,"height":100}"#);
        let binding = to(&small, Point::new(50.0, 100.0)).expect("a binding");
        let large = read(r#"{"type":"rectangle","id":"a","x":0,"y":0,"width":200,"height":200}"#);
        let at = point(&binding, &large);
        // Within the nudge the fraction takes to keep off the middle line.
        assert!((at.x - 100.0).abs() < 0.05, "x is {}", at.x);
        assert!((at.y - 200.0).abs() < 0.05, "y is {}", at.y);
    }

    #[test]
    fn a_binding_points_at_the_same_place_on_a_turned_shape() {
        let straight =
            read(r#"{"type":"rectangle","id":"a","x":0,"y":0,"width":100,"height":100}"#);
        let binding = to(&straight, Point::new(100.0, 50.0)).expect("a binding");
        let turned = read(
            r#"{"type":"rectangle","id":"a","x":0,"y":0,"width":100,"height":100,
                "angle":1.5707963268}"#,
        );
        let at = point(&binding, &turned);
        // A quarter turn takes the right edge to the bottom, within the nudge the fraction takes
        // to keep off the middle line.
        assert!((at.x - 50.0).abs() < 0.05, "x is {}", at.x);
        assert!((at.y - 100.0).abs() < 0.05, "y is {}", at.y);
    }

    #[test]
    fn an_orbiting_end_is_held_off_the_shape() {
        let shape = read(
            r#"{"type":"rectangle","id":"a","x":0,"y":0,"width":100,"height":100,
                "strokeWidth":2}"#,
        );
        let binding = to(&shape, Point::new(120.0, 50.0)).expect("a binding");
        assert_eq!(binding.mode, BindMode::Orbit);
        let held = end(&binding, &shape, Point::new(200.0, 50.0));
        let bare = point(&binding, &shape);
        assert!((held - bare).hypot() > 1e-6, "it was held off");
        assert!((held - bare).hypot() <= gap(&shape) + 1e-9);
    }

    #[test]
    fn an_end_inside_the_shape_is_exactly_where_it_says() {
        let shape = read(r#"{"type":"rectangle","id":"a","x":0,"y":0,"width":100,"height":100}"#);
        let binding = to(&shape, Point::new(50.0, 60.0)).expect("a binding");
        assert_eq!(binding.mode, BindMode::Inside);
        assert_eq!(
            end(&binding, &shape, Point::new(200.0, 50.0)),
            point(&binding, &shape)
        );
    }

    #[test]
    fn nothing_binds_to_something_that_cannot_be_bound_to() {
        let arrow = read(r#"{"type":"arrow","id":"a","points":[[0,0],[10,0]]}"#);
        assert!(to(&arrow, Point::ZERO).is_none());
        let gone = read(r#"{"type":"rectangle","id":"a","width":10,"height":10,"isDeleted":true}"#);
        assert!(to(&gone, Point::ZERO).is_none());
    }

    #[test]
    fn an_arrow_follows_the_shape_its_end_is_bound_to() {
        let shape = read(r#"{"type":"rectangle","id":"s","x":200,"y":0,"width":100,"height":100}"#);
        let arrow = read(
            r#"{"type":"arrow","id":"a","x":0,"y":50,"points":[[0,0],[100,0]],
                "endBinding":{"elementId":"s","fixedPoint":[0,0.5001],"mode":"inside"}}"#,
        );
        let moved = moved_points(&arrow, None, Some(&shape)).expect("it moved");
        assert_eq!(moved[0], Point::ZERO, "the first point stays the origin");
        // The end went to the shape's left edge, at 200.
        assert!((moved[1].x - 200.0).abs() < 0.05, "x is {}", moved[1].x);
    }

    #[test]
    fn an_arrow_bound_to_nothing_does_not_move() {
        let arrow = read(r#"{"type":"arrow","id":"a","points":[[0,0],[100,0]]}"#);
        assert!(moved_points(&arrow, None, None).is_none());
    }
}
