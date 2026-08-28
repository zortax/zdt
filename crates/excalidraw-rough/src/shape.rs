//! The shapes an Excalidraw element is drawn as.
//!
//! Each one draws its outline first and its fill second, because that is the order rough.js asks
//! its generator for numbers in, and the order decides where every later stroke lands. The list
//! that comes back is in painting order, which is the other way round.

use kurbo::{BezPath, PathEl, Point};

use crate::ops::{Drawable, OpSet, OpSetKind};
use crate::options::{FillStyle, Options};
use crate::random::Random;
use crate::renderer::{
    bezier_to, curve_ops_rough, double_line, ellipse_ops, ellipse_walk, linear_path_ops,
};
use crate::{fill, renderer};

/// One list of ops, as one ring.
///
/// A solid fill is the outline itself, filled — but the outline is drawn in several strokes, and a
/// ring per stroke would cancel itself out under the even-odd rule and leave the middle empty. So
/// every lift of the pen but the first is dropped, and what is left is one ring.
fn merged(ops: Vec<crate::ops::Op>) -> Vec<crate::ops::Op> {
    ops.into_iter()
        .enumerate()
        .filter(|(at, op)| *at == 0 || !matches!(op, crate::ops::Op::Move(_)))
        .map(|(_, op)| op)
        .collect()
}

/// One shape, and the fill it takes.
fn assembled(outline: OpSet, filling: Option<OpSet>) -> Drawable {
    let mut sets = Vec::with_capacity(2);
    if let Some(filling) = filling {
        sets.push(filling);
    }
    if !outline.is_empty() {
        sets.push(outline);
    }
    Drawable { sets }
}

/// The fill for a shape cut from `polygons`, when it has one.
fn filling(polygons: &[Vec<Point>], options: &Options, random: &mut Random) -> Option<OpSet> {
    if !options.filled {
        return None;
    }
    Some(if options.fill_style == FillStyle::Solid {
        fill::solid(polygons, options, random)
    } else {
        fill::pattern(polygons, options, random)
    })
}

/// A hand-drawn run of straight lines, closed when asked.
#[must_use]
pub fn polygon(points: &[Point], close: bool, options: &Options, random: &mut Random) -> Drawable {
    let outline = OpSet::from_ops(
        OpSetKind::Path,
        linear_path_ops(points, close, options, random),
    );
    let filling = filling(&[points.to_vec()], options, random);
    assembled(outline, filling)
}

/// A hand-drawn rectangle, its corners sharp.
#[must_use]
pub fn rectangle(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    options: &Options,
    random: &mut Random,
) -> Drawable {
    let points = [
        Point::new(x, y),
        Point::new(x + width, y),
        Point::new(x + width, y + height),
        Point::new(x, y + height),
    ];
    polygon(&points, true, options, random)
}

/// A hand-drawn ellipse, centred on `center`.
#[must_use]
pub fn ellipse(
    center: Point,
    width: f64,
    height: f64,
    options: &Options,
    random: &mut Random,
) -> Drawable {
    let walk = ellipse_walk(width, height, options, random);
    let (ops, core) = ellipse_ops(center, &walk, options, random);
    let outline = OpSet::from_ops(OpSetKind::Path, ops);

    let filling = if !options.filled {
        None
    } else if options.fill_style == FillStyle::Solid {
        // A solid ellipse is a second ellipse, filled. It is drawn again rather than reused, which
        // is what spends the random draws rough.js spends here.
        let (again, _) = ellipse_ops(center, &walk, options, random);
        Some(OpSet::from_ops(OpSetKind::FillPath, again))
    } else {
        Some(fill::pattern(&[core], options, random))
    };
    assembled(outline, filling)
}

/// A hand-drawn curve through `points`.
#[must_use]
pub fn curve(points: &[Point], options: &Options, random: &mut Random) -> Drawable {
    let outline = OpSet::from_ops(OpSetKind::Path, curve_ops_rough(points, options, random));
    let filling = if !options.filled {
        None
    } else if options.fill_style == FillStyle::Solid {
        // A solid curve's fill is the same curve, in one rougher stroke.
        let rougher = Options {
            disable_multi_stroke: true,
            roughness: if options.roughness == 0.0 {
                0.0
            } else {
                options.roughness + options.fill_shape_roughness_gain
            },
            ..options.clone()
        };
        // The same generator the outline was drawn with, carried on rather than started again:
        // the fill is the next thing this hand draws, not another hand.
        Some(OpSet::from_ops(
            OpSetKind::FillPath,
            merged(curve_ops_rough(points, &rougher, random)),
        ))
    } else {
        Some(fill::pattern(&[points.to_vec()], options, random))
    };
    assembled(outline, filling)
}

/// A hand-drawn run of straight lines, never closed and never filled.
#[must_use]
pub fn linear_path(points: &[Point], options: &Options, random: &mut Random) -> Drawable {
    Drawable {
        sets: vec![OpSet::from_ops(
            OpSetKind::Path,
            linear_path_ops(points, false, options, random),
        )],
    }
}

/// A hand-drawn version of an SVG path.
///
/// Only the four commands Excalidraw writes are read: a move, a line, a cubic and a close. A
/// quadratic is raised to a cubic first, which is what rough.js's normalisation does.
#[must_use]
pub fn path(d: &str, options: &Options, random: &mut Random) -> Drawable {
    let Ok(parsed) = BezPath::from_svg(d) else {
        return Drawable::default();
    };
    let outline = OpSet::from_ops(OpSetKind::Path, path_ops(&parsed, options, random));

    let filling = if !options.filled {
        None
    } else if options.fill_style == FillStyle::Solid {
        let rougher = Options {
            disable_multi_stroke: true,
            roughness: if options.roughness == 0.0 {
                0.0
            } else {
                options.roughness + options.fill_shape_roughness_gain
            },
            ..options.clone()
        };
        // The same generator the outline was drawn with, carried on.
        Some(OpSet::from_ops(
            OpSetKind::FillPath,
            merged(path_ops(&parsed, &rougher, random)),
        ))
    } else {
        Some(fill::pattern(&[flattened(&parsed)], options, random))
    };
    assembled(outline, filling)
}

/// The strokes one parsed path is drawn with.
fn path_ops(parsed: &BezPath, options: &Options, random: &mut Random) -> Vec<crate::ops::Op> {
    let mut ops = Vec::new();
    let mut current = Point::ZERO;
    let mut first = Point::ZERO;
    for element in parsed.elements() {
        match *element {
            PathEl::MoveTo(to) => {
                current = to;
                first = to;
            }
            PathEl::LineTo(to) => {
                ops.extend(double_line(current, to, options, false, random));
                current = to;
            }
            PathEl::QuadTo(c, to) => {
                // The cubic a quadratic is, so the hand that draws cubics draws this too.
                let c1 = current + (c - current) * (2.0 / 3.0);
                let c2 = to + (c - to) * (2.0 / 3.0);
                ops.extend(bezier_to(c1, c2, to, current, options, random));
                current = to;
            }
            PathEl::CurveTo(c1, c2, to) => {
                ops.extend(bezier_to(c1, c2, to, current, options, random));
                current = to;
            }
            PathEl::ClosePath => {
                ops.extend(double_line(current, first, options, false, random));
                current = first;
            }
        }
    }
    ops
}

/// The path as a run of points, for a fill to be cut from.
fn flattened(parsed: &BezPath) -> Vec<Point> {
    let mut points = Vec::new();
    kurbo::flatten(parsed.elements().iter().copied(), 0.5, |element| {
        if let PathEl::MoveTo(to) | PathEl::LineTo(to) = element {
            points.push(to);
        }
    });
    points
}

/// The outline a shape's fill would be cut from, for a caller that wants it separately.
#[must_use]
pub fn ellipse_outline(
    center: Point,
    width: f64,
    height: f64,
    options: &Options,
    random: &mut Random,
) -> Vec<Point> {
    let walk = ellipse_walk(width, height, options, random);
    let (_, core) = ellipse_ops(center, &walk, options, random);
    core
}

/// The outline of a hand-drawn curve, for a caller that only wants the strokes.
#[must_use]
pub fn curve_outline(points: &[Point], options: &Options, random: &mut Random) -> OpSet {
    renderer::curve(points, options, random)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> Options {
        Options {
            seed: 1_263_748_391,
            ..Options::default()
        }
    }

    #[test]
    fn an_unfilled_rectangle_is_one_outline() {
        let options = options();
        let mut random = options.random();
        let drawn = rectangle(0.0, 0.0, 100.0, 60.0, &options, &mut random);
        assert_eq!(drawn.sets.len(), 1);
        assert_eq!(drawn.sets[0].kind, OpSetKind::Path);
    }

    #[test]
    fn a_filled_rectangle_paints_its_inside_first() {
        let options = Options {
            filled: true,
            ..options()
        };
        let mut random = options.random();
        let drawn = rectangle(0.0, 0.0, 100.0, 60.0, &options, &mut random);
        assert_eq!(drawn.sets.len(), 2);
        assert_eq!(drawn.sets[0].kind, OpSetKind::FillSketch);
        assert_eq!(drawn.sets[1].kind, OpSetKind::Path);
    }

    #[test]
    fn the_same_seed_draws_the_same_rectangle() {
        let draw = || {
            let options = options();
            let mut random = options.random();
            rectangle(0.0, 0.0, 100.0, 60.0, &options, &mut random)
        };
        assert_eq!(draw(), draw());
    }

    #[test]
    fn a_different_seed_draws_a_different_rectangle() {
        let draw = |seed| {
            let options = Options {
                seed,
                ..Options::default()
            };
            let mut random = options.random();
            rectangle(0.0, 0.0, 100.0, 60.0, &options, &mut random)
        };
        assert_ne!(draw(1), draw(2));
    }

    #[test]
    fn a_path_is_drawn_by_the_same_hand_as_a_line() {
        let options = options();
        let mut random = options.random();
        let drawn = path(
            "M 10 0 L 90 0 Q 100 0, 100 10 L 100 50",
            &options,
            &mut random,
        );
        assert_eq!(drawn.sets.len(), 1);
        assert!(!drawn.sets[0].is_empty());
    }

    #[test]
    fn an_unreadable_path_draws_nothing() {
        let options = options();
        let mut random = options.random();
        assert!(path("not a path", &options, &mut random).sets.is_empty());
    }

    #[test]
    fn a_filled_ellipse_draws_its_inside_as_a_second_ellipse() {
        let options = Options {
            filled: true,
            fill_style: FillStyle::Solid,
            curve_fitting: 1.0,
            ..options()
        };
        let mut random = options.random();
        let drawn = ellipse(Point::new(50.0, 30.0), 100.0, 60.0, &options, &mut random);
        assert_eq!(drawn.sets[0].kind, OpSetKind::FillPath);
        assert_eq!(drawn.sets[1].kind, OpSetKind::Path);
    }
}
