//! The shape an element makes in its own space, before any hand draws it.
//!
//! This is the exact geometry: the rounded rectangle, the diamond, the ellipse, the run of points.
//! It is what a hit test measures against, what a fill is cut from, and what the drawing library is
//! handed to wobble.

use kurbo::{BezPath, Ellipse, Point, Rect, Shape as _};

use crate::element::{Data, Element, Kind};

use super::diamond_points;

/// How closely a curve is walked when it has to become points.
pub const FLATTEN_TOLERANCE: f64 = 0.25;

/// The exact outline of `element`, in its own space.
///
/// A kind with no closed outline — words, a picture, a pen stroke — answers its box, which is what
/// a selection and a hit test want from it.
#[must_use]
pub fn of(element: &Element) -> BezPath {
    let (width, height) = (element.width, element.height);
    match element.kind {
        Kind::Ellipse => Ellipse::new(
            Point::new(width / 2.0, height / 2.0),
            (width / 2.0, height / 2.0),
            0.0,
        )
        .to_path(FLATTEN_TOLERANCE),
        Kind::Diamond => diamond_path(element, width, height),
        Kind::Line | Kind::Arrow => linear_path(element),
        Kind::Freedraw => freedraw_path(element),
        _ => rectangle_path(element, width, height),
    }
}

/// The rounded rectangle a boxed element is.
fn rectangle_path(element: &Element, width: f64, height: f64) -> BezPath {
    let radius = corner_radius(element, width.min(height));
    let box_ = Rect::new(0.0, 0.0, width, height);
    if radius <= 0.0 {
        return box_.to_path(FLATTEN_TOLERANCE);
    }
    kurbo::RoundedRect::from_rect(box_, radius).to_path(FLATTEN_TOLERANCE)
}

/// The diamond, with its corners cut when it is round.
fn diamond_path(element: &Element, width: f64, height: f64) -> BezPath {
    let corners = diamond_points(width, height);
    let mut path = BezPath::new();
    let Some(roundness) = element.roundness else {
        path.move_to(corners[0]);
        for corner in &corners[1..] {
            path.line_to(*corner);
        }
        path.close_path();
        return path;
    };

    // One radius, never more than half of the edge it is cut from, so a narrow diamond's corners
    // do not curve past each other.
    let radius = roundness.radius(width.min(height) / 2.0);
    let toward = |from: Point, to: Point| {
        let along = to - from;
        let length = along.hypot();
        if length == 0.0 {
            from
        } else {
            from + along * (radius.min(length / 2.0) / length)
        }
    };

    for at in 0..4 {
        let corner = corners[at];
        let before = corners[(at + 3) % 4];
        let after = corners[(at + 1) % 4];
        if at == 0 {
            path.move_to(toward(corner, after));
        } else {
            path.line_to(toward(corner, before));
            path.quad_to(corner, toward(corner, after));
        }
    }
    // Round the first corner last, which is what closes the ring.
    path.line_to(toward(corners[0], corners[3]));
    path.quad_to(corners[0], toward(corners[0], corners[1]));
    path.close_path();
    path
}

/// The run of segments or curves a line or an arrow is.
fn linear_path(element: &Element) -> BezPath {
    let Data::Linear(linear) = &element.data else {
        return BezPath::new();
    };
    let mut path = BezPath::new();
    let Some(first) = linear.points.first() else {
        return path;
    };
    path.move_to(*first);

    // A round line is a curve through its points; a sharp one is the segments between them.
    if element.roundness.is_some() && linear.points.len() > 2 && !linear.elbowed {
        for point in catmull_rom(&linear.points) {
            path.curve_to(point.0, point.1, point.2);
        }
    } else {
        for point in &linear.points[1..] {
            path.line_to(*point);
        }
    }
    if linear.polygon {
        path.close_path();
    }
    path
}

/// The ring a pen stroke fills.
fn freedraw_path(element: &Element) -> BezPath {
    let Data::Freedraw(stroke) = &element.data else {
        return BezPath::new();
    };
    excalidraw_rough::Stroke {
        points: &stroke.points,
        pressures: &stroke.pressures,
        simulate_pressure: stroke.simulate_pressure,
        stroke_width: element.stroke_width,
        streamline: stroke.streamline,
        variability: stroke.variability,
    }
    .path()
}

/// The cubics a smooth run through `points` is made of.
///
/// This is the same uniform fit the drawing library uses, so a round line's exact outline and its
/// hand-drawn one agree about where it goes.
#[must_use]
pub fn catmull_rom(points: &[Point]) -> Vec<(Point, Point, Point)> {
    if points.len() < 2 {
        return Vec::new();
    }
    let mut walk = Vec::with_capacity(points.len() + 2);
    walk.push(points[0]);
    walk.extend_from_slice(points);
    walk.push(points[points.len() - 1]);

    let mut out = Vec::with_capacity(points.len());
    for at in 1..walk.len() - 2 {
        let (before, from, to, after) = (walk[at - 1], walk[at], walk[at + 1], walk[at + 2]);
        out.push((
            Point::new(
                from.x + (to.x - before.x) / 6.0,
                from.y + (to.y - before.y) / 6.0,
            ),
            Point::new(
                to.x + (from.x - after.x) / 6.0,
                to.y + (from.y - after.y) / 6.0,
            ),
            to,
        ));
    }
    out
}

/// The corner radius `element` cuts from a side of length `side`.
#[must_use]
pub fn corner_radius(element: &Element, side: f64) -> f64 {
    match element.roundness {
        Some(roundness) if element.kind.can_be_round() => roundness.radius(side),
        // A rounding on a kind that cannot be round is a file being odd, not an instruction.
        Some(_) | None => 0.0,
    }
}

/// `element`'s outline as a run of points, for a fill to be cut from.
#[must_use]
pub fn as_points(element: &Element) -> Vec<Point> {
    let mut points = Vec::new();
    kurbo::flatten(
        of(element).elements().iter().copied(),
        FLATTEN_TOLERANCE,
        |element| {
            if let kurbo::PathEl::MoveTo(to) | kurbo::PathEl::LineTo(to) = element {
                points.push(to);
            }
        },
    );
    points
}

#[cfg(test)]
mod tests {
    use kurbo::Shape as _;

    use super::*;
    use crate::element::Roundness as R;

    fn read(json: &str) -> Element {
        let value: serde_json::Value = serde_json::from_str(json).expect("valid JSON");
        crate::element::read(value.as_object().expect("an object")).expect("an element")
    }

    #[test]
    fn a_sharp_rectangle_is_its_box() {
        let held = read(r#"{"type":"rectangle","width":100,"height":50}"#);
        let bounds = of(&held).bounding_box();
        assert!((bounds.width() - 100.0).abs() < 1e-9);
        assert!((bounds.height() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn a_round_rectangle_still_fills_its_box() {
        let held = read(r#"{"type":"rectangle","width":220,"height":128,"roundness":{"type":3}}"#);
        assert_eq!(held.roundness, Some(R::Adaptive { value: None }));
        let bounds = of(&held).bounding_box();
        assert!((bounds.width() - 220.0).abs() < 1e-6);
        assert!((bounds.height() - 128.0).abs() < 1e-6);
    }

    #[test]
    fn an_ellipse_fills_its_box() {
        let held = read(r#"{"type":"ellipse","width":140,"height":140}"#);
        let bounds = of(&held).bounding_box();
        assert!((bounds.width() - 140.0).abs() < 1e-6);
    }

    #[test]
    fn a_sharp_diamond_reaches_every_side_of_its_box() {
        let bounds = of(&read(r#"{"type":"diamond","width":160,"height":110}"#)).bounding_box();
        assert!((bounds.width() - 160.0).abs() < 1.5);
        assert!((bounds.height() - 110.0).abs() < 1.5);
    }

    /// A cut corner pulls the point in, so a round diamond is a little smaller than its box and
    /// never larger.
    #[test]
    fn a_round_diamond_sits_inside_its_box() {
        let bounds = of(&read(
            r#"{"type":"diamond","width":160,"height":110,"roundness":{"type":2}}"#,
        ))
        .bounding_box();
        assert!(bounds.width() <= 160.0 + 1e-6);
        assert!(bounds.width() > 120.0, "it is still most of the box");
        assert!(bounds.height() <= 110.0 + 1e-6);
    }

    #[test]
    fn a_sharp_line_is_the_segments_between_its_points() {
        let held = read(r#"{"type":"line","points":[[0,0],[90,-60],[180,0]]}"#);
        let path = of(&held);
        assert_eq!(path.elements().len(), 3, "a move and two lines");
    }

    #[test]
    fn a_round_line_is_a_curve_through_them() {
        let held =
            read(r#"{"type":"line","points":[[0,0],[90,-60],[180,0]],"roundness":{"type":2}}"#);
        let path = of(&held);
        assert!(
            path.elements()
                .iter()
                .any(|element| matches!(element, kurbo::PathEl::CurveTo(..)))
        );
    }

    #[test]
    fn a_closed_line_is_a_ring() {
        let held = read(r#"{"type":"line","points":[[0,0],[50,0],[50,50],[0,50]],"polygon":true}"#);
        assert!(
            of(&held)
                .elements()
                .iter()
                .any(|element| matches!(element, kurbo::PathEl::ClosePath))
        );
    }

    #[test]
    fn a_pen_stroke_is_the_ring_around_where_the_pen_went() {
        let held =
            read(r#"{"type":"freedraw","points":[[0,0],[20,4],[40,0]],"simulatePressure":true}"#);
        let bounds = of(&held).bounding_box();
        assert!(bounds.width() > 40.0, "the ring is wider than the line");
    }

    #[test]
    fn a_rounding_on_a_kind_that_cannot_be_round_is_ignored() {
        let held = read(r#"{"type":"ellipse","width":100,"height":100,"roundness":{"type":2}}"#);
        assert!((corner_radius(&held, 100.0)).abs() < f64::EPSILON);
    }
}
