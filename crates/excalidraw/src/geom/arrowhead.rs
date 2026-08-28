//! What decorates the end of an arrow.
//!
//! A head is aimed along the line as it was actually drawn, not along the points it was drawn from:
//! a round arrow leaves its last point at an angle the points do not name. So the head is built
//! from the drawn curve, sampled a little way in from the end.

use kurbo::{BezPath, ParamCurve as _, Point, Shape as _, Vec2};

use crate::element::Arrowhead;

use super::{rotated, unit};

/// How far along the end curve the direction is read from.
///
/// Far enough in to be a direction, near enough to still be the end.
const SAMPLE: f64 = 0.3;

/// Which end of a line a head is on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum End {
    /// The one the line starts at.
    Start,
    /// The one it finishes at.
    End,
}

/// The tip of a head, and the two points its barbs come from.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Geometry {
    /// Where the head points.
    pub tip: Point,
    /// One barb.
    pub left: Point,
    /// The other.
    pub right: Point,
    /// The point the barbs fan out from, back along the line.
    pub base: Point,
    /// Which way the line runs into the tip.
    pub direction: Vec2,
    /// How large the head is drawn.
    pub size: f64,
}

/// Where the head at `end` of `drawn` sits.
///
/// `segment` is how long the last run of the line is, which caps the head so a short arrow does not
/// end in a head longer than itself. Answers nothing when the line is too short to have a direction.
#[must_use]
pub fn geometry(
    drawn: &BezPath,
    end: End,
    head: Arrowhead,
    segment: f64,
    offset: f64,
) -> Option<Geometry> {
    let (tip, near) = ends(drawn, end)?;
    let along = tip - near;
    if along.hypot() < f64::EPSILON {
        return None;
    }
    let direction = unit(along);

    let size = head.size();
    // A diamond is drawn from its middle, so it is allowed half as much of the run.
    let share = match head {
        Arrowhead::Diamond | Arrowhead::DiamondOutline => 0.25,
        _ => 0.5,
    };
    let reach = size.min(segment * share);

    let tip = tip - direction * (reach * offset);
    let base = tip - direction * reach;
    let angle = head.angle().to_radians();
    Some(Geometry {
        tip,
        left: rotated(base, tip, -angle),
        right: rotated(base, tip, angle),
        base,
        direction,
        size: reach,
    })
}

/// The end point of `drawn`, and a point a little way back along it.
fn ends(drawn: &BezPath, end: End) -> Option<(Point, Point)> {
    let segments: Vec<kurbo::PathSeg> = drawn.segments().collect();
    if segments.is_empty() {
        return None;
    }
    let segment = match end {
        End::Start => *segments.first()?,
        End::End => *segments.last()?,
    };
    let (tip, near) = match end {
        End::Start => (segment.eval(0.0), segment.eval(SAMPLE)),
        End::End => (segment.eval(1.0), segment.eval(1.0 - SAMPLE)),
    };
    Some((tip, near))
}

/// The geometry a head of this kind is drawn as.
#[derive(Clone, PartialEq, Debug)]
pub struct Drawn {
    /// The strokes it is made of.
    pub strokes: Vec<BezPath>,
    /// The ring it fills, when it has one.
    pub filled: Option<BezPath>,
    /// Whether that ring takes the line's colour or the page's.
    pub fill_is_stroke: bool,
}

/// The head `head` is, from `geometry`.
#[must_use]
pub fn draw(head: Arrowhead, at: &Geometry) -> Drawn {
    let mut strokes = Vec::new();
    let mut filled = None;
    let mut fill_is_stroke = true;

    let bar = |along: f64| -> BezPath {
        let center = at.tip - at.direction * (at.size * along);
        let across = Vec2::new(-at.direction.y, at.direction.x) * (at.size / 2.0);
        let mut path = BezPath::new();
        path.move_to(center - across);
        path.line_to(center + across);
        path
    };
    let ring =
        |center: Point, radius: f64| -> BezPath { kurbo::Circle::new(center, radius).to_path(0.1) };
    let barbs = || -> Vec<BezPath> {
        [at.left, at.right]
            .into_iter()
            .map(|barb| {
                let mut path = BezPath::new();
                path.move_to(barb);
                path.line_to(at.tip);
                path
            })
            .collect()
    };

    match head {
        Arrowhead::Arrow | Arrowhead::Bar => strokes.extend(barbs()),
        Arrowhead::Circle | Arrowhead::CircleOutline => {
            let radius = (at.tip - at.base).hypot() / 2.0;
            let center = at.tip - at.direction * radius;
            filled = Some(ring(center, radius));
            fill_is_stroke = head == Arrowhead::Circle;
        }
        Arrowhead::Triangle | Arrowhead::TriangleOutline => {
            let mut path = BezPath::new();
            path.move_to(at.tip);
            path.line_to(at.left);
            path.line_to(at.right);
            path.close_path();
            filled = Some(path);
            fill_is_stroke = head == Arrowhead::Triangle;
        }
        Arrowhead::Diamond | Arrowhead::DiamondOutline => {
            let far = at.tip - at.direction * (at.size * 2.0);
            let mut path = BezPath::new();
            path.move_to(at.tip);
            path.line_to(at.left);
            path.line_to(far);
            path.line_to(at.right);
            path.close_path();
            filled = Some(path);
            fill_is_stroke = head == Arrowhead::Diamond;
        }
        Arrowhead::CardinalityOne => strokes.push(bar(1.0)),
        Arrowhead::CardinalityMany => strokes.extend(barbs()),
        Arrowhead::CardinalityOneOrMany => {
            strokes.extend(barbs());
            strokes.push(bar(1.25));
        }
        Arrowhead::CardinalityExactlyOne => {
            strokes.push(bar(1.0));
            strokes.push(bar(1.5));
        }
        Arrowhead::CardinalityZeroOrOne => {
            let radius = at.size * 0.4;
            strokes.push(bar(1.5));
            filled = Some(ring(at.tip - at.direction * (at.size * 0.6), radius));
            fill_is_stroke = false;
        }
        Arrowhead::CardinalityZeroOrMany => {
            strokes.extend(barbs());
            let radius = at.size * 0.4;
            filled = Some(ring(at.tip - at.direction * (at.size * 1.6), radius));
            fill_is_stroke = false;
        }
    }

    Drawn {
        strokes,
        filled,
        fill_is_stroke,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line() -> BezPath {
        let mut path = BezPath::new();
        path.move_to(Point::new(0.0, 0.0));
        path.line_to(Point::new(100.0, 0.0));
        path
    }

    #[test]
    fn a_head_points_the_way_the_line_runs() {
        let at = geometry(&line(), End::End, Arrowhead::Arrow, 100.0, 0.0).expect("a head");
        assert!((at.tip - Point::new(100.0, 0.0)).hypot() < 1e-9);
        assert!((at.direction.x - 1.0).abs() < 1e-9);
        assert!(at.base.x < at.tip.x, "the barbs fan out backwards");
    }

    #[test]
    fn the_head_at_the_start_points_the_other_way() {
        let at = geometry(&line(), End::Start, Arrowhead::Arrow, 100.0, 0.0).expect("a head");
        assert!((at.tip - Point::new(0.0, 0.0)).hypot() < 1e-9);
        assert!((at.direction.x + 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_short_line_gets_a_head_no_longer_than_itself() {
        let mut short = BezPath::new();
        short.move_to(Point::new(0.0, 0.0));
        short.line_to(Point::new(10.0, 0.0));
        let at = geometry(&short, End::End, Arrowhead::Arrow, 10.0, 0.0).expect("a head");
        assert!(at.size <= 5.0, "half the run, not the full head size");
    }

    #[test]
    fn a_line_with_no_length_has_no_head() {
        let mut nothing = BezPath::new();
        nothing.move_to(Point::ZERO);
        nothing.line_to(Point::ZERO);
        assert!(geometry(&nothing, End::End, Arrowhead::Arrow, 0.0, 0.0).is_none());
        assert!(geometry(&BezPath::new(), End::End, Arrowhead::Arrow, 0.0, 0.0).is_none());
    }

    #[test]
    fn a_filled_head_fills_and_an_open_one_does_not_take_the_lines_colour() {
        let at = geometry(&line(), End::End, Arrowhead::Triangle, 100.0, 0.0).expect("a head");
        assert!(draw(Arrowhead::Triangle, &at).fill_is_stroke);
        assert!(!draw(Arrowhead::TriangleOutline, &at).fill_is_stroke);
        assert!(draw(Arrowhead::Arrow, &at).filled.is_none());
        assert_eq!(draw(Arrowhead::Arrow, &at).strokes.len(), 2);
    }

    #[test]
    fn every_head_draws_something() {
        let at = geometry(&line(), End::End, Arrowhead::Arrow, 100.0, 0.0).expect("a head");
        for head in [
            Arrowhead::Arrow,
            Arrowhead::Bar,
            Arrowhead::Circle,
            Arrowhead::CircleOutline,
            Arrowhead::Triangle,
            Arrowhead::TriangleOutline,
            Arrowhead::Diamond,
            Arrowhead::DiamondOutline,
            Arrowhead::CardinalityOne,
            Arrowhead::CardinalityMany,
            Arrowhead::CardinalityOneOrMany,
            Arrowhead::CardinalityExactlyOne,
            Arrowhead::CardinalityZeroOrOne,
            Arrowhead::CardinalityZeroOrMany,
        ] {
            let drawn = draw(head, &at);
            assert!(
                !drawn.strokes.is_empty() || drawn.filled.is_some(),
                "{head:?} draws nothing"
            );
        }
    }
}
