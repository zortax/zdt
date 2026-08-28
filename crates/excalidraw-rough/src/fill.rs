//! How the inside of a shape is drawn.
//!
//! Every style but `solid` is a set of parallel lines cut from the shape by a scan line, then drawn
//! by the same hand that draws the outline. The scan runs along the horizontal, so the polygon is
//! turned under it and the lines are turned back.

use kurbo::Point;

use crate::ops::{Op, OpSet, OpSetKind};
use crate::options::{FillStyle, Options};
use crate::random::Random;
use crate::renderer::{double_line, offset_opt};

/// One cut line, from end to end.
type Line = (Point, Point);

/// `points` turned `degrees` about the origin.
fn rotated(points: &[Point], degrees: f64) -> Vec<Point> {
    let angle = degrees.to_radians();
    let (sin, cos) = angle.sin_cos();
    points
        .iter()
        .map(|point| Point::new(point.x * cos - point.y * sin, point.x * sin + point.y * cos))
        .collect()
}

/// One edge of a polygon, as the scan reads it.
struct Edge {
    /// Where it starts, down the page.
    ymin: f64,
    /// Where it ends.
    ymax: f64,
    /// Where it is, across the page, at `ymin`.
    x: f64,
    /// How far across it moves per line down.
    islope: f64,
}

/// The horizontal lines that cut `polygons` every `gap`, stepping `step` at a time.
fn straight_lines(polygons: &[Vec<Point>], gap: f64, step: f64) -> Vec<Line> {
    let mut edges: Vec<Edge> = Vec::new();
    for polygon in polygons {
        let mut vertices = polygon.clone();
        match (vertices.first(), vertices.last()) {
            (Some(first), Some(last)) if first != last => vertices.push(*first),
            (None, _) => continue,
            _ => {}
        }
        if vertices.len() <= 2 {
            continue;
        }
        for pair in vertices.windows(2) {
            let (p1, p2) = (pair[0], pair[1]);
            if p1.y == p2.y {
                continue;
            }
            let ymin = p1.y.min(p2.y);
            edges.push(Edge {
                ymin,
                ymax: p1.y.max(p2.y),
                x: if ymin == p1.y { p1.x } else { p2.x },
                islope: (p2.x - p1.x) / (p2.y - p1.y),
            });
        }
    }
    if edges.is_empty() {
        return Vec::new();
    }
    edges.sort_by(|a, b| {
        a.ymin
            .partial_cmp(&b.ymin)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| {
                a.ymax
                    .partial_cmp(&b.ymax)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    let mut lines = Vec::new();
    let mut active: Vec<(f64, Edge)> = Vec::new();
    let mut y = edges[0].ymin;
    let mut iteration = 0u64;
    let mut waiting = std::collections::VecDeque::from(edges);

    while !active.is_empty() || !waiting.is_empty() {
        while waiting.front().is_some_and(|edge| edge.ymin <= y) {
            let edge = waiting.pop_front().expect("just looked at");
            active.push((y, edge));
        }
        active.retain(|(_, edge)| edge.ymax > y);
        active.sort_by(|a, b| {
            a.1.x
                .partial_cmp(&b.1.x)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // With a step of one, only every `gap`th line is kept. With a larger step, the step is
        // already the gap and every line is kept.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let gap_lines = gap.max(1.0) as u64;
        if step != 1.0 || iteration.is_multiple_of(gap_lines) {
            for pair in active.chunks_exact(2) {
                lines.push((
                    Point::new(pair[0].1.x.round(), y),
                    Point::new(pair[1].1.x.round(), y),
                ));
            }
        }

        y += step;
        for (_, edge) in &mut active {
            edge.x += step * edge.islope;
        }
        iteration += 1;
        // A degenerate polygon whose edges never close would otherwise scan forever.
        if iteration > 1_000_000 {
            break;
        }
    }
    lines
}

/// The lines that cut `polygons` at this angle and gap.
fn hachure_lines(
    polygons: &[Vec<Point>],
    options: &Options,
    hachure_angle: f64,
    random: &mut Random,
) -> Vec<Line> {
    let angle = hachure_angle + 90.0;
    let gap = options.hachure_gap();

    // A rough shape skips most of the lines, so the fill reads as strokes rather than as a block.
    let mut step = 1.0;
    if options.roughness >= 1.0 && random.next() > 0.7 {
        step = gap;
    }

    let turned: Vec<Vec<Point>> = if angle == 0.0 {
        polygons.to_vec()
    } else {
        polygons
            .iter()
            .map(|polygon| rotated(polygon, angle))
            .collect()
    };
    let lines = straight_lines(&turned, gap, step);
    if angle == 0.0 {
        return lines;
    }
    lines
        .into_iter()
        .map(|(from, to)| {
            let back = rotated(&[from, to], -angle);
            (back[0], back[1])
        })
        .collect()
}

/// Each line, drawn by the same hand as the outline.
fn drawn(lines: &[Line], options: &Options, random: &mut Random) -> Vec<Op> {
    let mut ops = Vec::new();
    for (from, to) in lines {
        ops.extend(double_line(*from, *to, options, true, random));
    }
    ops
}

/// The inside of `polygons`, as a filled outline near where it was asked for.
#[must_use]
pub fn solid(polygons: &[Vec<Point>], options: &Options, random: &mut Random) -> OpSet {
    let wander = options.max_randomness_offset;
    let mut ops = Vec::new();
    for points in polygons {
        if points.len() <= 2 {
            continue;
        }
        let at = |point: &Point, random: &mut Random| {
            let dx = offset_opt(wander, options, 1.0, random);
            let dy = offset_opt(wander, options, 1.0, random);
            Point::new(point.x + dx, point.y + dy)
        };
        ops.push(Op::Move(at(&points[0], random)));
        for point in &points[1..] {
            ops.push(Op::Line(at(point, random)));
        }
    }
    OpSet::from_ops(OpSetKind::FillPath, ops)
}

/// The inside of `polygons`, as strokes.
#[must_use]
pub fn pattern(polygons: &[Vec<Point>], options: &Options, random: &mut Random) -> OpSet {
    let gap = options.hachure_gap();
    let ops = match options.fill_style {
        FillStyle::Solid => return solid(polygons, options, random),
        FillStyle::CrossHatch => {
            let mut ops = drawn(
                &hachure_lines(polygons, options, options.hachure_angle, random),
                options,
                random,
            );
            ops.extend(drawn(
                &hachure_lines(polygons, options, options.hachure_angle + 90.0, random),
                options,
                random,
            ));
            ops
        }
        FillStyle::ZigZag => {
            let lines = hachure_lines(polygons, options, options.hachure_angle, random);
            let angle = options.hachure_angle.to_radians();
            let (dx, dy) = (gap * 0.5 * angle.cos(), gap * 0.5 * angle.sin());
            let mut zigzag = Vec::with_capacity(lines.len() * 2);
            for (from, to) in lines {
                if (to - from).hypot() == 0.0 {
                    continue;
                }
                zigzag.push((Point::new(from.x - dx, from.y + dy), to));
                zigzag.push((Point::new(from.x + dx, from.y - dy), to));
            }
            drawn(&zigzag, options, random)
        }
        FillStyle::Dashed => {
            let lines = hachure_lines(polygons, options, options.hachure_angle, random);
            dashed(&lines, options, random)
        }
        FillStyle::Dots => {
            let dotted = Options {
                hachure_angle: 0.0,
                ..options.clone()
            };
            let lines = hachure_lines(polygons, &dotted, 0.0, random);
            dots(&lines, &dotted, random)
        }
        FillStyle::ZigZagLine => {
            let spread = if options.zigzag_offset < 0.0 {
                gap
            } else {
                options.zigzag_offset
            };
            let wider = Options {
                hachure_gap: gap + spread,
                ..options.clone()
            };
            let lines = hachure_lines(polygons, &wider, wider.hachure_angle, random);
            zigzag_line(&lines, spread, options, random)
        }
        FillStyle::Hachure => drawn(
            &hachure_lines(polygons, options, options.hachure_angle, random),
            options,
            random,
        ),
    };
    OpSet::from_ops(OpSetKind::FillSketch, ops)
}

/// Each line, broken into dashes.
fn dashed(lines: &[Line], options: &Options, random: &mut Random) -> Vec<Op> {
    let gap = options.hachure_gap();
    let offset = if options.dash_offset < 0.0 {
        gap
    } else {
        options.dash_offset
    };
    let space = if options.dash_gap < 0.0 {
        gap
    } else {
        options.dash_gap
    };

    let mut ops = Vec::new();
    for (from, to) in lines {
        let length = (*to - *from).hypot();
        let count = (length / (offset + space)).floor();
        let total = (count + 1.0).mul_add(offset, count * space);
        let mut along = ((length - total) / 2.0).max(0.0) / length;
        if length < 4.0 {
            continue;
        }
        let (mut start, mut end) = (along, along);
        while end < 1.0 {
            end = (start + offset / length).min(1.0);
            let a = Point::new(
                from.x + (to.x - from.x) * start,
                from.y + (to.y - from.y) * start,
            );
            let b = Point::new(
                from.x + (to.x - from.x) * end,
                from.y + (to.y - from.y) * end,
            );
            ops.extend(double_line(a, b, options, true, random));
            start = end + space / length;
            end = start;
            along = start;
            if along >= 1.0 {
                break;
            }
        }
    }
    ops
}

/// Each line, as a row of dots.
fn dots(lines: &[Line], options: &Options, random: &mut Random) -> Vec<Op> {
    let gap = options.hachure_gap().max(0.1);
    let radius = (options.fill_weight().max(0.1)) / 2.0;
    let wander = gap / 4.0;

    let mut ops = Vec::new();
    for (from, to) in lines {
        let length = (*to - *from).hypot();
        let count = (length / gap).round();
        let spacing = (length - count * gap) / 2.0;
        if count < 1.0 {
            continue;
        }
        let along = (*to - *from) / length;
        for step in 0..=(count as i64) {
            #[allow(clippy::cast_precision_loss)]
            let at = spacing + (step as f64) * gap;
            let dx = offset_opt(wander, options, 1.0, random);
            let dy = offset_opt(wander, options, 1.0, random);
            let center = Point::new(from.x + along.x * at + dx, from.y + along.y * at + dy);
            ops.extend(circle_ops(center, radius));
        }
    }
    ops
}

/// One filled circle, as four cubics.
fn circle_ops(center: Point, radius: f64) -> Vec<Op> {
    // The distance a cubic's controls sit from the ends to trace a quarter circle.
    const KAPPA: f64 = 0.552_284_749_830_793_4;
    let k = radius * KAPPA;
    let (cx, cy) = (center.x, center.y);
    vec![
        Op::Move(Point::new(cx + radius, cy)),
        Op::Curve(
            Point::new(cx + radius, cy + k),
            Point::new(cx + k, cy + radius),
            Point::new(cx, cy + radius),
        ),
        Op::Curve(
            Point::new(cx - k, cy + radius),
            Point::new(cx - radius, cy + k),
            Point::new(cx - radius, cy),
        ),
        Op::Curve(
            Point::new(cx - radius, cy - k),
            Point::new(cx - k, cy - radius),
            Point::new(cx, cy - radius),
        ),
        Op::Curve(
            Point::new(cx + k, cy - radius),
            Point::new(cx + radius, cy - k),
            Point::new(cx + radius, cy),
        ),
    ]
}

/// Each line, as a zigzag that swings `spread` either side of it.
fn zigzag_line(lines: &[Line], spread: f64, options: &Options, random: &mut Random) -> Vec<Op> {
    let swing = (2.0 * spread * spread).sqrt();
    let mut ops = Vec::new();
    for (from, to) in lines {
        let length = (*to - *from).hypot();
        if length == 0.0 {
            continue;
        }
        let count = (length / (2.0 * spread)).round();
        if count < 1.0 {
            continue;
        }
        let along = (*to - *from) / length;
        let alpha = along.y.atan2(along.x);
        let step = length / count;

        let mut at = *from;
        let mut up = true;
        for _ in 0..(count as i64) {
            let angle = alpha
                + if up {
                    std::f64::consts::FRAC_PI_4
                } else {
                    -std::f64::consts::FRAC_PI_4
                };
            let next = Point::new(at.x + along.x * step, at.y + along.y * step);
            let peak = Point::new(at.x + swing * angle.cos(), at.y + swing * angle.sin());
            ops.extend(double_line(at, peak, options, true, random));
            ops.extend(double_line(peak, next, options, true, random));
            at = next;
            up = !up;
        }
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(size: f64) -> Vec<Vec<Point>> {
        vec![vec![
            Point::new(0.0, 0.0),
            Point::new(size, 0.0),
            Point::new(size, size),
            Point::new(0.0, size),
        ]]
    }

    #[test]
    fn a_hachured_square_is_cut_into_lines() {
        let options = Options {
            seed: 1,
            filled: true,
            stroke_width: 2.0,
            ..Options::default()
        };
        let mut random = options.random();
        let set = pattern(&square(100.0), &options, &mut random);
        assert_eq!(set.kind, OpSetKind::FillSketch);
        assert!(!set.is_empty(), "a hundred-pixel square has room for lines");
    }

    #[test]
    fn cross_hatch_draws_more_than_hachure() {
        let base = Options {
            seed: 1,
            filled: true,
            stroke_width: 2.0,
            ..Options::default()
        };
        let count = |style| {
            let options = Options {
                fill_style: style,
                ..base.clone()
            };
            let mut random = options.random();
            pattern(&square(100.0), &options, &mut random).ops.len()
        };
        assert!(count(FillStyle::CrossHatch) > count(FillStyle::Hachure));
    }

    #[test]
    fn a_solid_fill_is_the_outline_itself() {
        let options = Options {
            seed: 1,
            filled: true,
            fill_style: FillStyle::Solid,
            ..Options::default()
        };
        let mut random = options.random();
        let set = pattern(&square(40.0), &options, &mut random);
        assert_eq!(set.kind, OpSetKind::FillPath);
        assert_eq!(set.ops.len(), 4, "a move and three lines");
    }
}
