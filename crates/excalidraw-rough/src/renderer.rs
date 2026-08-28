//! The strokes a shape is drawn with.
//!
//! Each routine here is rough.js's, and the order it draws its random numbers in is part of it: a
//! line that asked for its offsets in another order would wander somewhere else, and a file drawn
//! elsewhere would not match. Every function therefore takes the generator and threads it through
//! in the order rough.js does.

use kurbo::Point;

use crate::ops::{Op, OpSet, OpSetKind};
use crate::options::Options;
use crate::random::Random;

/// A number in `[min, max)`, scaled by how rough the shape is.
pub(crate) fn offset(min: f64, max: f64, options: &Options, gain: f64, random: &mut Random) -> f64 {
    options.roughness * gain * (random.next() * (max - min) + min)
}

/// The same, either side of nothing.
pub(crate) fn offset_opt(x: f64, options: &Options, gain: f64, random: &mut Random) -> f64 {
    offset(-x, x, options, gain, random)
}

/// How much of a line's roughness is used, which falls away as it gets longer.
fn roughness_gain(length: f64) -> f64 {
    if length < 200.0 {
        1.0
    } else if length > 500.0 {
        0.4
    } else {
        -0.0016_668 * length + 1.233_334
    }
}

/// One stroke from `from` to `to`.
///
/// `mover` asks for the opening move, and `overlay` is the second of the pair, which wanders half
/// as far so the two read as one hand-drawn line. A preserved vertex spends no random draw, which
/// is what rough.js's short circuit does and what every later stroke's position depends on.
fn line(
    from: Point,
    to: Point,
    options: &Options,
    mover: bool,
    overlay: bool,
    random: &mut Random,
) -> Vec<Op> {
    let length_sq = (from.x - to.x).powi(2) + (from.y - to.y).powi(2);
    let length = length_sq.sqrt();
    let gain = roughness_gain(length);

    let mut wander = options.max_randomness_offset;
    if wander * wander * 100.0 > length_sq {
        wander = length / 10.0;
    }
    if overlay {
        wander /= 2.0;
    }

    let diverge = 0.2 + random.next() * 0.2;
    let bow_x = options.bowing * options.max_randomness_offset * (to.y - from.y) / 200.0;
    let bow_y = options.bowing * options.max_randomness_offset * (from.x - to.x) / 200.0;
    let bow_x = offset_opt(bow_x, options, gain, random);
    let bow_y = offset_opt(bow_y, options, gain, random);

    let preserve = options.preserve_vertices;
    let jitter = |random: &mut Random| offset_opt(wander, options, gain, random);
    let vertex = |point: Point, random: &mut Random| {
        if preserve {
            point
        } else {
            let dx = jitter(random);
            let dy = jitter(random);
            Point::new(point.x + dx, point.y + dy)
        }
    };

    let mut ops = Vec::with_capacity(2);
    if mover {
        ops.push(Op::Move(vertex(from, random)));
    }
    let c1 = Point::new(
        bow_x + from.x + (to.x - from.x) * diverge + jitter(random),
        bow_y + from.y + (to.y - from.y) * diverge + jitter(random),
    );
    let c2 = Point::new(
        bow_x + from.x + 2.0 * (to.x - from.x) * diverge + jitter(random),
        bow_y + from.y + 2.0 * (to.y - from.y) * diverge + jitter(random),
    );
    ops.push(Op::Curve(c1, c2, vertex(to, random)));
    ops
}

/// The two strokes one hand-drawn line is made of.
pub(crate) fn double_line(
    from: Point,
    to: Point,
    options: &Options,
    filling: bool,
    random: &mut Random,
) -> Vec<Op> {
    let single = if filling {
        options.disable_multi_stroke_fill
    } else {
        options.disable_multi_stroke
    };
    let mut ops = line(from, to, options, true, false, random);
    if !single {
        ops.extend(line(from, to, options, true, true, random));
    }
    ops
}

/// A run of straight lines through `points`, closed when asked.
pub(crate) fn linear_path_ops(
    points: &[Point],
    close: bool,
    options: &Options,
    random: &mut Random,
) -> Vec<Op> {
    let count = points.len();
    if count < 3 {
        if count == 2 {
            return double_line(points[0], points[1], options, false, random);
        }
        if count == 1 {
            // rough.js draws nothing for a lone point; a caller wanting a dot asks for one.
            return Vec::new();
        }
        return Vec::new();
    }

    let mut ops = Vec::new();
    for pair in points.windows(2) {
        ops.extend(double_line(pair[0], pair[1], options, false, random));
    }
    if close {
        ops.extend(double_line(
            points[count - 1],
            points[0],
            options,
            false,
            random,
        ));
    }
    ops
}

/// The outline of a run of straight lines.
#[must_use]
pub fn linear_path(points: &[Point], close: bool, options: &Options, random: &mut Random) -> OpSet {
    OpSet::from_ops(
        OpSetKind::Path,
        linear_path_ops(points, close, options, random),
    )
}

/// A cubic through an already-padded run of points.
///
/// The run has its first and last points doubled, which is what makes the curve reach its ends
/// rather than starting inside them. [`curve_with_offset`] is what pads it.
pub(crate) fn curve_ops(points: &[Point], options: &Options, random: &mut Random) -> Vec<Op> {
    let count = points.len();
    let mut ops = Vec::new();
    if count > 3 {
        let tension = 1.0 - options.curve_tightness;
        ops.push(Op::Move(points[1]));
        for at in 1..count - 2 {
            let (before, from, to, after) =
                (points[at - 1], points[at], points[at + 1], points[at + 2]);
            ops.push(Op::Curve(
                Point::new(
                    from.x + (tension * to.x - tension * before.x) / 6.0,
                    from.y + (tension * to.y - tension * before.y) / 6.0,
                ),
                Point::new(
                    to.x + (tension * from.x - tension * after.x) / 6.0,
                    to.y + (tension * from.y - tension * after.y) / 6.0,
                ),
                to,
            ));
        }
    } else if count == 3 {
        ops.push(Op::Move(points[1]));
        ops.push(Op::Curve(points[1], points[2], points[2]));
    } else if count == 2 {
        ops = double_line(points[0], points[1], options, false, random);
    }
    ops
}

/// The same, with every point moved somewhere near where it was asked for.
fn curve_with_offset(
    points: &[Point],
    wander: f64,
    options: &Options,
    random: &mut Random,
) -> Vec<Op> {
    if points.is_empty() {
        return Vec::new();
    }
    let jitter = |point: Point, random: &mut Random| {
        let dx = offset_opt(wander, options, 1.0, random);
        let dy = offset_opt(wander, options, 1.0, random);
        Point::new(point.x + dx, point.y + dy)
    };

    let last = points.len() - 1;
    let mut walk = Vec::with_capacity(points.len() + 2);
    walk.push(jitter(points[0], random));
    walk.push(jitter(points[0], random));
    for (at, point) in points.iter().enumerate().skip(1) {
        walk.push(jitter(*point, random));
        if at == last {
            walk.push(jitter(*point, random));
        }
    }
    curve_ops(&walk, options, random)
}

/// The two strokes a hand-drawn curve is made of.
pub(crate) fn curve_ops_rough(points: &[Point], options: &Options, random: &mut Random) -> Vec<Op> {
    let mut ops = curve_with_offset(
        points,
        1.0 * (1.0 + options.roughness * 0.2),
        options,
        random,
    );
    if !options.disable_multi_stroke {
        // The second stroke is drawn from the next seed, so the two wander apart rather than
        // lying on top of each other.
        let second = options.next_seed();
        let mut random = second.random();
        ops.extend(curve_with_offset(
            points,
            1.5 * (1.0 + second.roughness * 0.22),
            &second,
            &mut random,
        ));
    }
    ops
}

/// The outline of a hand-drawn curve.
#[must_use]
pub fn curve(points: &[Point], options: &Options, random: &mut Random) -> OpSet {
    OpSet::from_ops(OpSetKind::Path, curve_ops_rough(points, options, random))
}

/// How an ellipse is walked: the size of one step, and the radii to walk at.
#[derive(Clone, Copy, Debug)]
pub struct EllipseWalk {
    /// The angle one step turns through.
    pub increment: f64,
    /// The horizontal radius, already wandered.
    pub rx: f64,
    /// The vertical radius, already wandered.
    pub ry: f64,
}

/// The walk an ellipse of this size is drawn with.
#[must_use]
pub fn ellipse_walk(
    width: f64,
    height: f64,
    options: &Options,
    random: &mut Random,
) -> EllipseWalk {
    let psq = (std::f64::consts::PI
        * 2.0
        * (((width / 2.0).powi(2) + (height / 2.0).powi(2)) / 2.0).sqrt())
    .sqrt();
    let step_count = options
        .curve_step_count
        .max((options.curve_step_count / 200.0_f64.sqrt()) * psq)
        .ceil();
    let increment = (std::f64::consts::PI * 2.0) / step_count;
    let mut rx = (width / 2.0).abs();
    let mut ry = (height / 2.0).abs();
    // One is exactly, which is what an Excalidraw ellipse asks for.
    let fitting = 1.0 - options.curve_fitting;
    rx += offset_opt(rx * fitting, options, 1.0, random);
    ry += offset_opt(ry * fitting, options, 1.0, random);
    EllipseWalk { increment, rx, ry }
}

/// The points one pass around an ellipse visits.
///
/// The first list is what the curve is drawn through, with the extra points at each end that make
/// it close; the second is the ellipse itself, which is what a fill is cut from.
fn ellipse_points(
    center: Point,
    walk: &EllipseWalk,
    wander: f64,
    overlap: f64,
    options: &Options,
    random: &mut Random,
) -> (Vec<Point>, Vec<Point>) {
    let (cx, cy) = (center.x, center.y);
    let (rx, ry) = (walk.rx, walk.ry);
    let mut core = Vec::new();
    let mut all = Vec::new();

    // An exact ellipse is walked four times as finely and never wanders.
    if options.roughness == 0.0 {
        let increment = walk.increment / 4.0;
        all.push(Point::new(
            cx + rx * (-increment).cos(),
            cy + ry * (-increment).sin(),
        ));
        let mut angle = 0.0;
        while angle <= std::f64::consts::PI * 2.0 {
            let at = Point::new(cx + rx * angle.cos(), cy + ry * angle.sin());
            core.push(at);
            all.push(at);
            angle += increment;
        }
        all.push(Point::new(cx + rx, cy));
        all.push(Point::new(
            cx + rx * increment.cos(),
            cy + ry * increment.sin(),
        ));
        return (all, core);
    }

    let rad_offset = offset_opt(0.5, options, 1.0, random) - std::f64::consts::FRAC_PI_2;
    let at = |angle: f64, scale: f64, random: &mut Random| {
        let dx = offset_opt(wander, options, 1.0, random);
        let dy = offset_opt(wander, options, 1.0, random);
        Point::new(
            dx + cx + scale * rx * angle.cos(),
            dy + cy + scale * ry * angle.sin(),
        )
    };

    all.push(at(rad_offset - walk.increment, 0.9, random));
    let end = std::f64::consts::PI * 2.0 + rad_offset - 0.01;
    let mut angle = rad_offset;
    while angle < end {
        let point = at(angle, 1.0, random);
        core.push(point);
        all.push(point);
        angle += walk.increment;
    }
    all.push(at(
        rad_offset + std::f64::consts::PI * 2.0 + overlap * 0.5,
        1.0,
        random,
    ));
    all.push(at(rad_offset + overlap, 0.98, random));
    all.push(at(rad_offset + overlap * 0.5, 0.9, random));
    (all, core)
}

/// A hand-drawn ellipse, and the points its fill is cut from.
pub(crate) fn ellipse_ops(
    center: Point,
    walk: &EllipseWalk,
    options: &Options,
    random: &mut Random,
) -> (Vec<Op>, Vec<Point>) {
    let overlap = walk.increment
        * offset(
            0.1,
            offset(0.4, 1.0, options, 1.0, random),
            options,
            1.0,
            random,
        );
    let (path, core) = ellipse_points(center, walk, 1.0, overlap, options, random);
    let mut ops = curve_ops(&path, options, random);
    if !options.disable_multi_stroke && options.roughness != 0.0 {
        let (second, _) = ellipse_points(center, walk, 1.5, 0.0, options, random);
        ops.extend(curve_ops(&second, options, random));
    }
    (ops, core)
}

/// The outline of a hand-drawn ellipse, and the points its fill is cut from.
#[must_use]
pub fn ellipse(
    center: Point,
    width: f64,
    height: f64,
    options: &Options,
    random: &mut Random,
) -> (OpSet, Vec<Point>) {
    let walk = ellipse_walk(width, height, options, random);
    let (ops, core) = ellipse_ops(center, &walk, options, random);
    (OpSet::from_ops(OpSetKind::Path, ops), core)
}

/// A cubic drawn by hand: the same curve, twice, near where it was asked for.
///
/// `from` is where the pen already is. The endpoint's wander is drawn before the control points',
/// which is the order rough.js asks in and therefore where every later stroke lands.
pub(crate) fn bezier_to(
    c1: Point,
    c2: Point,
    to: Point,
    from: Point,
    options: &Options,
    random: &mut Random,
) -> Vec<Op> {
    let wanders = [
        options.max_randomness_offset,
        options.max_randomness_offset + 0.3,
    ];
    let passes = if options.disable_multi_stroke { 1 } else { 2 };
    let preserve = options.preserve_vertices;
    let mut ops = Vec::with_capacity(passes * 2);

    for (pass, wander) in wanders.into_iter().enumerate().take(passes) {
        // The first pass starts exactly where the pen is; the second wanders, unless the vertex
        // is preserved.
        if pass == 0 || preserve {
            ops.push(Op::Move(from));
        } else {
            let dx = offset_opt(wanders[0], options, 1.0, random);
            let dy = offset_opt(wanders[0], options, 1.0, random);
            ops.push(Op::Move(Point::new(from.x + dx, from.y + dy)));
        }

        let end = if preserve {
            to
        } else {
            let dx = offset_opt(wander, options, 1.0, random);
            let dy = offset_opt(wander, options, 1.0, random);
            Point::new(to.x + dx, to.y + dy)
        };
        let a = Point::new(
            c1.x + offset_opt(wander, options, 1.0, random),
            c1.y + offset_opt(wander, options, 1.0, random),
        );
        let b = Point::new(
            c2.x + offset_opt(wander, options, 1.0, random),
            c2.y + offset_opt(wander, options, 1.0, random),
        );
        ops.push(Op::Curve(a, b, end));
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(roughness: f64) -> Options {
        Options {
            seed: 1,
            roughness,
            ..Options::default()
        }
    }

    #[test]
    fn a_line_is_a_move_and_two_curves() {
        let options = options(1.0);
        let mut random = options.random();
        let ops = double_line(
            Point::new(0.0, 0.0),
            Point::new(100.0, 0.0),
            &options,
            false,
            &mut random,
        );
        assert_eq!(ops.len(), 4, "two strokes, each a move and a curve");
        assert!(matches!(ops[0], Op::Move(_)));
        assert!(matches!(ops[1], Op::Curve(..)));
        assert!(matches!(ops[2], Op::Move(_)));
    }

    #[test]
    fn one_stroke_is_asked_for_when_multi_stroke_is_off() {
        let options = Options {
            disable_multi_stroke: true,
            ..options(1.0)
        };
        let mut random = options.random();
        let ops = double_line(
            Point::new(0.0, 0.0),
            Point::new(100.0, 0.0),
            &options,
            false,
            &mut random,
        );
        assert_eq!(ops.len(), 2);
    }

    /// Preserved vertices are the whole reason an elbow arrow's corners meet.
    #[test]
    fn preserved_vertices_land_exactly_where_they_were_asked_for() {
        let options = Options {
            preserve_vertices: true,
            ..options(2.0)
        };
        let mut random = options.random();
        let ops = double_line(
            Point::new(10.0, 20.0),
            Point::new(110.0, 20.0),
            &options,
            false,
            &mut random,
        );
        assert_eq!(ops[0], Op::Move(Point::new(10.0, 20.0)));
        assert_eq!(ops[1].end(), Point::new(110.0, 20.0));
    }

    #[test]
    fn a_curve_reaches_the_points_it_was_fitted_to() {
        let options = Options {
            roughness: 0.0,
            disable_multi_stroke: true,
            ..options(0.0)
        };
        let points = [
            Point::new(0.0, 0.0),
            Point::new(50.0, 40.0),
            Point::new(100.0, 0.0),
        ];
        let mut random = options.random();
        let ops = curve_ops_rough(&points, &options, &mut random);
        assert_eq!(ops.first().map(Op::end), Some(points[0]));
        assert_eq!(ops.last().map(Op::end), Some(points[2]));
    }

    #[test]
    fn an_exact_ellipse_keeps_its_radii() {
        let options = Options {
            roughness: 0.0,
            curve_fitting: 1.0,
            disable_multi_stroke: true,
            ..options(0.0)
        };
        let mut random = options.random();
        let walk = ellipse_walk(200.0, 100.0, &options, &mut random);
        assert!((walk.rx - 100.0).abs() < f64::EPSILON);
        assert!((walk.ry - 50.0).abs() < f64::EPSILON);
    }
}
