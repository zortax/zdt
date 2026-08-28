//! The path an elbowed arrow takes.
//!
//! An elbowed arrow runs in right angles, and its corners are filleted so the turn reads as one
//! stroke rather than two. The fillet is never larger than half of either run, so a short segment
//! does not curve past its own end.

use kurbo::{BezPath, Point};

/// How large a corner is cut, at most.
pub const CORNER_RADIUS: f64 = 16.0;

/// The path through `points`, with its corners cut.
#[must_use]
pub fn path(points: &[Point], radius: f64) -> BezPath {
    let mut path = BezPath::new();
    let Some(first) = points.first() else {
        return path;
    };
    if points.len() < 3 {
        path.move_to(*first);
        for point in points.iter().skip(1) {
            path.line_to(*point);
        }
        return path;
    }

    path.move_to(*first);
    for at in 1..points.len() - 1 {
        let (before, corner, after) = (points[at - 1], points[at], points[at + 1]);
        let into = corner - before;
        let out_of = after - corner;
        let cut = radius.min(into.hypot() / 2.0).min(out_of.hypot() / 2.0);
        if cut <= 0.0 {
            path.line_to(corner);
            continue;
        }
        let entry = corner - into * (cut / into.hypot());
        let exit = corner + out_of * (cut / out_of.hypot());
        path.line_to(entry);
        path.quad_to(corner, exit);
    }
    path.line_to(points[points.len() - 1]);
    path
}

/// The same path, as the notation the drawing library reads.
///
/// The library is handed a path rather than a run of points because a filleted corner is a curve,
/// and a run of points has no way to say so.
#[must_use]
pub fn as_svg(points: &[Point], radius: f64) -> String {
    path(points, radius).to_svg()
}

#[cfg(test)]
mod tests {
    use kurbo::{PathEl, Shape as _};

    use super::*;

    fn corner() -> Vec<Point> {
        vec![
            Point::new(0.0, 0.0),
            Point::new(60.0, 0.0),
            Point::new(60.0, 80.0),
            Point::new(120.0, 80.0),
        ]
    }

    #[test]
    fn every_corner_is_cut() {
        let held = path(&corner(), CORNER_RADIUS);
        let curves = held
            .elements()
            .iter()
            .filter(|element| matches!(element, PathEl::QuadTo(..)))
            .count();
        assert_eq!(curves, 2, "the two turns");
    }

    #[test]
    fn the_path_still_starts_and_ends_where_it_was_asked_for() {
        let points = corner();
        let held = path(&points, CORNER_RADIUS);
        let bounds = held.bounding_box();
        assert!((bounds.x0).abs() < 1e-9);
        assert!((bounds.x1 - 120.0).abs() < 1e-9);
        assert!((bounds.y1 - 80.0).abs() < 1e-9);
    }

    #[test]
    fn a_short_run_is_not_cut_past_its_own_end() {
        let points = [
            Point::new(0.0, 0.0),
            Point::new(4.0, 0.0),
            Point::new(4.0, 4.0),
        ];
        let held = path(&points, CORNER_RADIUS);
        let bounds = held.bounding_box();
        assert!(bounds.width() <= 4.0 + 1e-9);
        assert!(bounds.height() <= 4.0 + 1e-9);
    }

    #[test]
    fn two_points_are_one_straight_run() {
        let held = path(&[Point::ZERO, Point::new(10.0, 0.0)], CORNER_RADIUS);
        assert_eq!(held.elements().len(), 2);
    }
}
