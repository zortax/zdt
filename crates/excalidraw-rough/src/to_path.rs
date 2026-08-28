//! Turning drawn ops into geometry something can paint.

use kurbo::{BezPath, Point};

use crate::ops::{Drawable, Op, OpSet, OpSetKind};

/// The geometry one list of ops draws.
#[must_use]
pub fn of_ops(ops: &[Op]) -> BezPath {
    let mut path = BezPath::new();
    let mut open = false;
    for op in ops {
        match *op {
            Op::Move(to) => {
                path.move_to(to);
                open = true;
            }
            Op::Line(to) if open => path.line_to(to),
            Op::Curve(c1, c2, to) if open => path.curve_to(c1, c2, to),
            // An op before any move has nowhere to start from. rough.js never emits one; a
            // hand-written op list might.
            Op::Line(to) | Op::Curve(_, _, to) => {
                path.move_to(to);
                open = true;
            }
        }
    }
    path
}

/// The geometry one set draws.
#[must_use]
pub fn of_set(set: &OpSet) -> BezPath {
    of_ops(&set.ops)
}

/// Every set of `drawable`, in painting order, with what each is for.
#[must_use]
pub fn of_drawable(drawable: &Drawable) -> Vec<(OpSetKind, BezPath)> {
    drawable
        .sets
        .iter()
        .map(|set| (set.kind, of_set(set)))
        .collect()
}

/// A closed run of points, for a shape that is drawn exactly.
#[must_use]
pub fn of_points(points: &[Point], close: bool) -> BezPath {
    let mut path = BezPath::new();
    let Some(first) = points.first() else {
        return path;
    };
    path.move_to(*first);
    for point in &points[1..] {
        path.line_to(*point);
    }
    if close {
        path.close_path();
    }
    path
}

#[cfg(test)]
mod tests {
    use kurbo::Shape as _;

    use super::*;

    #[test]
    fn a_move_and_a_curve_become_a_path() {
        let ops = [
            Op::Move(Point::new(0.0, 0.0)),
            Op::Curve(
                Point::new(10.0, 0.0),
                Point::new(20.0, 10.0),
                Point::new(30.0, 10.0),
            ),
        ];
        let path = of_ops(&ops);
        assert_eq!(path.elements().len(), 2);
        assert!(path.bounding_box().width() > 0.0);
    }

    #[test]
    fn an_empty_list_draws_nothing() {
        assert!(of_ops(&[]).is_empty());
    }
}
