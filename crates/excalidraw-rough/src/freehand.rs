//! The outline a freehand stroke is filled with.
//!
//! A pen whose width follows how hard it was pressed cannot be a stroked line, because its width
//! changes along it. What is drawn is the outline of the whole stroke, filled. This is
//! perfect-freehand, ported: the input is pulled towards a smooth line, a radius is taken at each
//! point from its pressure, and the two sides plus a cap at each end are walked into one closed
//! ring.
//!
//! Excalidraw always asks for a round cap at each end and never for a taper, so those are what this
//! draws and the taper branches are left out.

use kurbo::{BezPath, Point, Vec2};

/// How much wider than the stroke width a pressure-varying stroke is drawn.
pub const SIZE_FACTOR: f64 = 4.25;
/// How much pressure narrows it.
pub const THINNING: f64 = 0.6;
/// How much its edges are softened.
pub const SMOOTHING: f64 = 0.5;
/// How much a constant-width stroke is widened instead.
pub const CONSTANT_SIZE_FACTOR: f64 = 1.4;
/// How much the input is pulled towards a smooth line when the element does not say.
pub const DEFAULT_STREAMLINE: f64 = 0.5;

/// How fast simulated pressure may change from one point to the next.
const RATE_OF_PRESSURE_CHANGE: f64 = 0.275;
/// A half turn, nudged past itself so a cap's last step is not dropped by a rounding error.
const FIXED_PI: f64 = std::f64::consts::PI + 0.000_1;
/// How many steps a start cap or a corner is walked in.
const CAP_STEPS: f64 = 13.0;
/// How many an end cap is.
const END_CAP_STEPS: f64 = 29.0;

/// Ease out on a sine, which is what Excalidraw maps pressure through.
fn ease_out_sine(t: f64) -> f64 {
    (t * std::f64::consts::FRAC_PI_2).sin()
}

/// How wide the pen is at this pressure.
fn stroke_radius(size: f64, thinning: f64, pressure: f64) -> f64 {
    size * ease_out_sine(0.5 - thinning * (0.5 - pressure))
}

/// The vector a quarter turn from `v`.
fn per(v: Vec2) -> Vec2 {
    Vec2::new(v.y, -v.x)
}

/// `v`, one unit long. A vector of no length stays as it is.
fn unit(v: Vec2) -> Vec2 {
    let length = v.hypot();
    if length == 0.0 { v } else { v / length }
}

/// `from` moved `t` of the way to `to`.
fn lerp(from: Point, to: Point, t: f64) -> Point {
    from + (to - from) * t
}

/// `at` turned `angle` about `center`.
fn rotated(at: Point, center: Point, angle: f64) -> Point {
    let (sin, cos) = angle.sin_cos();
    let d = at - center;
    Point::new(
        d.x * cos - d.y * sin + center.x,
        d.x * sin + d.y * cos + center.y,
    )
}

/// One point the pen was at, once the input has been smoothed.
#[derive(Clone, Copy, Debug)]
struct StrokePoint {
    /// Where it is.
    point: Point,
    /// How hard the pen was pressed.
    pressure: f64,
    /// Which way the pen came from.
    vector: Vec2,
    /// How far it is from the point before.
    distance: f64,
    /// How far along the whole stroke it is.
    running_length: f64,
}

/// How wide a stroke is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Variability {
    /// Pressure narrows it. The original freehand look.
    #[default]
    Variable,
    /// One width the whole way along.
    Constant,
}

/// One freehand stroke, as it is stored.
#[derive(Clone, Debug)]
pub struct Stroke<'a> {
    /// Where the pen went, in the element's own coordinates.
    pub points: &'a [Point],
    /// How hard it was pressed at each point, when the device said.
    pub pressures: &'a [f64],
    /// Whether to read pressure from how fast the pen moved instead.
    pub simulate_pressure: bool,
    /// How wide the element is drawn.
    pub stroke_width: f64,
    /// How much the input is pulled towards a smooth line.
    pub streamline: f64,
    /// How the width varies along it.
    pub variability: Variability,
}

impl Stroke<'_> {
    /// How wide the pen is, and how much pressure narrows it.
    fn size_and_thinning(&self) -> (f64, f64) {
        match self.variability {
            Variability::Variable => (self.stroke_width * SIZE_FACTOR, THINNING),
            // A constant stroke is the same centreline at a narrower, unvarying width. Excalidraw
            // draws this one with a laser-pointer geometry whose difference the width hides.
            Variability::Constant => (self.stroke_width * CONSTANT_SIZE_FACTOR, 0.0),
        }
    }

    /// The pen's path with its pressures, as the smoothing reads it.
    fn input(&self) -> Vec<(Point, Option<f64>)> {
        self.points
            .iter()
            .enumerate()
            .map(|(at, point)| {
                let pressure = if self.simulate_pressure {
                    None
                } else {
                    self.pressures.get(at).copied()
                };
                (*point, pressure)
            })
            .collect()
    }

    /// The smoothed centreline, which is what a boundary-sensitive caller wants.
    #[must_use]
    pub fn centreline(&self) -> Vec<Point> {
        let (size, _) = self.size_and_thinning();
        stroke_points(&self.input(), size, self.streamline)
            .into_iter()
            .map(|held| held.point)
            .collect()
    }

    /// The outline of this stroke, to be filled.
    #[must_use]
    pub fn outline(&self) -> Vec<Point> {
        let (size, thinning) = self.size_and_thinning();
        let walked = stroke_points(&self.input(), size, self.streamline);
        outline_points(&walked, size, thinning, SMOOTHING, self.simulate_pressure)
    }

    /// The same outline, as geometry, closed and smoothed through the midpoints.
    #[must_use]
    pub fn path(&self) -> BezPath {
        outline_path(&self.outline())
    }
}

/// The input, pulled towards a smooth line and thinned of points too close together.
fn stroke_points(input: &[(Point, Option<f64>)], size: f64, streamline: f64) -> Vec<StrokePoint> {
    if input.is_empty() {
        return Vec::new();
    }
    let t = 0.15 + (1.0 - streamline) * 0.85;

    let mut points: Vec<(Point, Option<f64>)> = input.to_vec();
    if points.len() == 2 {
        // Two points are too few to smooth, so four more are made up along the line between them.
        // They carry no pressure of their own, so each is drawn at the middle of the range.
        let to = points[1].0;
        points.truncate(1);
        for step in 1..5 {
            points.push((lerp(input[0].0, to, f64::from(step) / 4.0), None));
        }
    }
    if points.len() == 1 {
        let (at, pressure) = points[0];
        points.push((Point::new(at.x + 1.0, at.y + 1.0), pressure));
    }

    let mut walked = vec![StrokePoint {
        point: points[0].0,
        pressure: points[0].1.unwrap_or(0.25),
        vector: Vec2::new(1.0, 1.0),
        distance: 0.0,
        running_length: 0.0,
    }];

    let last = points.len() - 1;
    let mut reached_minimum = false;
    let mut running_length = 0.0;
    let mut previous = walked[0];

    for (at, (point, pressure)) in points.iter().enumerate().skip(1) {
        // The last point of a finished stroke is taken as it was drawn, so the stroke ends where
        // the pen was lifted.
        let point = if at == last {
            *point
        } else {
            lerp(previous.point, *point, t)
        };
        if point == previous.point {
            continue;
        }
        let distance = (point - previous.point).hypot();
        running_length += distance;
        // A stroke shorter than the pen is wide is one mark, not a line.
        if at < last && !reached_minimum {
            if running_length < size {
                continue;
            }
            reached_minimum = true;
        }
        previous = StrokePoint {
            point,
            pressure: pressure.unwrap_or(0.5),
            vector: unit(previous.point - point),
            distance,
            running_length,
        };
        walked.push(previous);
    }

    walked[0].vector = walked.get(1).map_or(Vec2::ZERO, |held| held.vector);
    walked
}

/// The ring around a smoothed centreline.
fn outline_points(
    points: &[StrokePoint],
    size: f64,
    thinning: f64,
    smoothing: f64,
    simulate_pressure: bool,
) -> Vec<Point> {
    if points.is_empty() || size <= 0.0 {
        return Vec::new();
    }
    let total_length = points[points.len() - 1].running_length;
    let min_distance = (size * smoothing).powi(2);

    // The pressure the stroke starts at, averaged over its first few points so a line does not
    // begin fat.
    let mut previous_pressure = points.iter().take(10).fold(points[0].pressure, |held, at| {
        let mut pressure = at.pressure;
        if simulate_pressure {
            let quick = (at.distance / size).min(1.0);
            let slow = (1.0 - quick).min(1.0);
            pressure = (held + (slow - held) * (quick * RATE_OF_PRESSURE_CHANGE)).min(1.0);
        }
        (held + pressure) / 2.0
    });

    let mut radius = stroke_radius(size, thinning, points[points.len() - 1].pressure);
    let mut first_radius: Option<f64> = None;
    let mut previous_vector = points[0].vector;
    let mut left_last = points[0].point;
    let mut right_last = points[0].point;
    let mut left: Vec<Point> = Vec::new();
    let mut right: Vec<Point> = Vec::new();
    let mut previous_was_sharp = false;

    let count = points.len();
    for (at, held) in points.iter().enumerate() {
        // The last few units of the stroke are drawn by the end cap, not by the sides.
        if at < count - 1 && total_length - held.running_length < 3.0 {
            continue;
        }

        let mut pressure = held.pressure;
        if thinning == 0.0 {
            radius = size / 2.0;
        } else {
            if simulate_pressure {
                let quick = (held.distance / size).min(1.0);
                let slow = (1.0 - quick).min(1.0);
                pressure = (previous_pressure
                    + (slow - previous_pressure) * (quick * RATE_OF_PRESSURE_CHANGE))
                    .min(1.0);
            }
            radius = stroke_radius(size, thinning, pressure);
        }
        if first_radius.is_none() {
            first_radius = Some(radius);
        }
        let radius = radius.max(0.01);

        let next_vector = points[if at < count - 1 { at + 1 } else { at }].vector;
        let next_dot = if at < count - 1 {
            held.vector.dot(next_vector)
        } else {
            1.0
        };
        let sharp_here = held.vector.dot(previous_vector) < 0.0 && !previous_was_sharp;
        let sharp_next = next_dot < 0.0;

        // A corner the pen turned back on is walked round rather than mitred.
        if sharp_here || sharp_next {
            let offset = per(previous_vector) * radius;
            let step = 1.0 / CAP_STEPS;
            let mut t = 0.0;
            while t <= 1.0 {
                left_last = rotated(held.point - offset, held.point, FIXED_PI * t);
                left.push(left_last);
                right_last = rotated(held.point + offset, held.point, FIXED_PI * -t);
                right.push(right_last);
                t += step;
            }
            previous_was_sharp = sharp_next;
            continue;
        }
        previous_was_sharp = false;

        if at == count - 1 {
            let offset = per(held.vector) * radius;
            left.push(held.point - offset);
            right.push(held.point + offset);
            continue;
        }

        // Between the way in and the way out, so the ring does not kink at every point.
        let between = next_vector + (held.vector - next_vector) * next_dot;
        let offset = per(between) * radius;
        let l = held.point - offset;
        if at <= 1 || (left_last - l).hypot2() > min_distance {
            left.push(l);
            left_last = l;
        }
        let r = held.point + offset;
        if at <= 1 || (right_last - r).hypot2() > min_distance {
            right.push(r);
            right_last = r;
        }
        previous_pressure = pressure;
        previous_vector = held.vector;
    }

    let first = points[0].point;
    let last = if count > 1 {
        points[count - 1].point
    } else {
        Point::new(first.x + 1.0, first.y + 1.0)
    };

    // A stroke of one point is a dot: a ring around where the pen was put down.
    if count == 1 {
        let start = first + unit(per(first - last)) * -(first_radius.unwrap_or(radius));
        let step = 1.0 / CAP_STEPS;
        let mut dot = Vec::new();
        let mut t = step;
        while t <= 1.0 {
            dot.push(rotated(start, first, FIXED_PI * 2.0 * t));
            t += step;
        }
        return dot;
    }

    let mut start_cap = Vec::new();
    let step = 1.0 / CAP_STEPS;
    let mut t = step;
    while t <= 1.0 {
        start_cap.push(rotated(right[0], first, FIXED_PI * t));
        t += step;
    }

    let mut end_cap = Vec::new();
    let direction = per(-points[count - 1].vector);
    let start = last + direction * radius;
    let step = 1.0 / END_CAP_STEPS;
    let mut t = step;
    while t < 1.0 {
        end_cap.push(rotated(start, last, FIXED_PI * 3.0 * t));
        t += step;
    }

    let mut ring = left;
    ring.extend(end_cap);
    ring.extend(right.into_iter().rev());
    ring.extend(start_cap);
    ring
}

/// A freehand outline as a closed, smoothed path.
///
/// Each pair of outline points becomes a quadratic through the point between them, which is what
/// takes the corners off a dense ring.
#[must_use]
pub fn outline_path(points: &[Point]) -> BezPath {
    let mut path = BezPath::new();
    let count = points.len();
    if count == 0 {
        return path;
    }
    let middle = |a: Point, b: Point| Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);

    path.move_to(points[0]);
    for at in 0..count {
        let point = points[at];
        let next = points[(at + 1) % count];
        path.quad_to(point, middle(point, next));
    }
    path.line_to(points[0]);
    path.close_path();
    path
}

#[cfg(test)]
mod tests {
    use kurbo::Shape as _;

    use super::*;

    fn stroke(points: &[Point]) -> Stroke<'_> {
        Stroke {
            points,
            pressures: &[],
            simulate_pressure: true,
            stroke_width: 1.0,
            streamline: DEFAULT_STREAMLINE,
            variability: Variability::Variable,
        }
    }

    #[test]
    fn a_drawn_line_has_an_outline_around_it() {
        let points = [
            Point::new(0.0, 0.0),
            Point::new(20.0, 4.0),
            Point::new(40.0, 0.0),
            Point::new(60.0, 10.0),
        ];
        let outline = stroke(&points).outline();
        assert!(
            outline.len() > points.len(),
            "the outline goes down one side and back up the other"
        );
        let bounds = outline_path(&outline).bounding_box();
        assert!(bounds.width() > 55.0 && bounds.height() > 5.0);
    }

    #[test]
    fn a_stroke_with_no_points_draws_nothing() {
        assert!(stroke(&[]).outline().is_empty());
        assert!(stroke(&[]).path().is_empty());
    }

    #[test]
    fn a_single_point_is_drawn_as_a_dot() {
        let points = [Point::new(5.0, 5.0)];
        let outline = stroke(&points).outline();
        assert!(!outline.is_empty(), "a tap leaves a mark");
        let bounds = outline_path(&outline).bounding_box();
        assert!(bounds.width() > 0.0 && bounds.height() > 0.0);
    }

    #[test]
    fn recorded_pressure_is_used_when_it_is_not_simulated() {
        let points = [
            Point::new(0.0, 0.0),
            Point::new(20.0, 0.0),
            Point::new(40.0, 0.0),
        ];
        let hard = Stroke {
            points: &points,
            pressures: &[1.0, 1.0, 1.0],
            simulate_pressure: false,
            ..stroke(&points)
        };
        let soft = Stroke {
            pressures: &[0.1, 0.1, 0.1],
            ..hard.clone()
        };
        let height = |stroke: &Stroke<'_>| outline_path(&stroke.outline()).bounding_box().height();
        assert!(
            height(&hard) > height(&soft),
            "pressing harder draws a wider stroke"
        );
    }

    #[test]
    fn a_constant_stroke_does_not_narrow_with_pressure() {
        let points = [
            Point::new(0.0, 0.0),
            Point::new(20.0, 0.0),
            Point::new(40.0, 0.0),
        ];
        let height = |pressures: &[f64]| {
            let stroke = Stroke {
                points: &points,
                pressures,
                simulate_pressure: false,
                stroke_width: 2.0,
                streamline: DEFAULT_STREAMLINE,
                variability: Variability::Constant,
            };
            outline_path(&stroke.outline()).bounding_box().height()
        };
        assert!((height(&[1.0, 1.0, 1.0]) - height(&[0.1, 0.1, 0.1])).abs() < 1e-9);
    }
}
