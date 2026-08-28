//! Element attributes as geometry: outlines, and the `transform` attribute.

use zgui::elements::kurbo::{self, Shape as _};

use super::{SvgAttr, SvgTag};

/// The value of `name`, when the element has it.
pub fn attr<'a>(attrs: &'a [SvgAttr], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|held| held.name == name)
        .map(|held| held.value.as_str())
}

/// The value of `name` as a number, or `fallback`.
pub fn number(attrs: &[SvgAttr], name: &str, fallback: f64) -> f64 {
    attr(attrs, name)
        .and_then(|value| value.trim().trim_end_matches("px").trim().parse().ok())
        .unwrap_or(fallback)
}

/// The element's outline in its own coordinates, when its attributes describe one.
pub fn outline(tag: SvgTag, attrs: &[SvgAttr]) -> Option<kurbo::BezPath> {
    /// How closely curves follow the true arc. Screen handles need no better.
    const TOLERANCE: f64 = 0.1;

    match tag {
        SvgTag::Path => attr(attrs, "d").and_then(|d| kurbo::BezPath::from_svg(d).ok()),
        SvgTag::Rect => {
            let (x, y) = (number(attrs, "x", 0.0), number(attrs, "y", 0.0));
            let (width, height) = (number(attrs, "width", 0.0), number(attrs, "height", 0.0));
            if width <= 0.0 || height <= 0.0 {
                return None;
            }
            let rect = kurbo::Rect::new(x, y, x + width, y + height);
            let rx = number(attrs, "rx", number(attrs, "ry", 0.0));
            Some(if rx > 0.0 {
                kurbo::RoundedRect::from_rect(rect, rx).to_path(TOLERANCE)
            } else {
                rect.to_path(TOLERANCE)
            })
        }
        SvgTag::Circle => {
            let center = (number(attrs, "cx", 0.0), number(attrs, "cy", 0.0));
            let r = number(attrs, "r", 0.0);
            (r > 0.0).then(|| kurbo::Circle::new(center, r).to_path(TOLERANCE))
        }
        SvgTag::Ellipse => {
            let center = (number(attrs, "cx", 0.0), number(attrs, "cy", 0.0));
            let (rx, ry) = (number(attrs, "rx", 0.0), number(attrs, "ry", 0.0));
            (rx > 0.0 && ry > 0.0)
                .then(|| kurbo::Ellipse::new(center, (rx, ry), 0.0).to_path(TOLERANCE))
        }
        SvgTag::Line => {
            let mut path = kurbo::BezPath::new();
            path.move_to((number(attrs, "x1", 0.0), number(attrs, "y1", 0.0)));
            path.line_to((number(attrs, "x2", 0.0), number(attrs, "y2", 0.0)));
            Some(path)
        }
        SvgTag::Polyline | SvgTag::Polygon => {
            let held = points(attr(attrs, "points")?);
            let (first, rest) = held.split_first()?;
            let mut path = kurbo::BezPath::new();
            path.move_to(*first);
            for point in rest {
                path.line_to(*point);
            }
            if tag == SvgTag::Polygon {
                path.close_path();
            }
            Some(path)
        }
    }
}

/// A `points` attribute as coordinate pairs. A trailing odd number is dropped.
pub fn points(value: &str) -> Vec<kurbo::Point> {
    let numbers: Vec<f64> = value
        .split(|held: char| held.is_ascii_whitespace() || held == ',')
        .filter(|held| !held.is_empty())
        .filter_map(|held| held.parse().ok())
        .collect();
    numbers
        .chunks_exact(2)
        .map(|pair| kurbo::Point::new(pair[0], pair[1]))
        .collect()
}

/// A `transform` attribute as one affine. An unreadable function reads as identity.
pub fn transform(value: &str) -> kurbo::Affine {
    let mut result = kurbo::Affine::IDENTITY;
    let mut rest = value.trim();
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim().trim_start_matches(',').trim();
        let Some(close) = rest[open..].find(')') else {
            break;
        };
        let arguments: Vec<f64> = rest[open + 1..open + close]
            .split(|held: char| held.is_ascii_whitespace() || held == ',')
            .filter(|held| !held.is_empty())
            .filter_map(|held| held.parse().ok())
            .collect();
        result *= function(name, &arguments);
        rest = &rest[open + close + 1..];
    }
    result
}

/// One transform function.
fn function(name: &str, arguments: &[f64]) -> kurbo::Affine {
    let argument = |at: usize, fallback: f64| arguments.get(at).copied().unwrap_or(fallback);
    match (name, arguments.len()) {
        ("translate", 1 | 2) => kurbo::Affine::translate((argument(0, 0.0), argument(1, 0.0))),
        ("scale", 1) => kurbo::Affine::scale(argument(0, 1.0)),
        ("scale", 2) => kurbo::Affine::scale_non_uniform(argument(0, 1.0), argument(1, 1.0)),
        ("rotate", 1) => kurbo::Affine::rotate(argument(0, 0.0).to_radians()),
        ("rotate", 3) => {
            let (cx, cy) = (argument(1, 0.0), argument(2, 0.0));
            kurbo::Affine::translate((cx, cy))
                * kurbo::Affine::rotate(argument(0, 0.0).to_radians())
                * kurbo::Affine::translate((-cx, -cy))
        }
        ("skewX", 1) => kurbo::Affine::skew(argument(0, 0.0).to_radians().tan(), 0.0),
        ("skewY", 1) => kurbo::Affine::skew(0.0, argument(0, 0.0).to_radians().tan()),
        ("matrix", 6) => kurbo::Affine::new([
            arguments[0],
            arguments[1],
            arguments[2],
            arguments[3],
            arguments[4],
            arguments[5],
        ]),
        _ => kurbo::Affine::IDENTITY,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_transform_function_reads() {
        let one = transform("translate(10, 20) scale(2)");
        assert_eq!(
            one * kurbo::Point::new(1.0, 1.0),
            kurbo::Point::new(12.0, 22.0)
        );

        let spaced = transform("translate(10 20)");
        assert_eq!(spaced * kurbo::Point::ZERO, kurbo::Point::new(10.0, 20.0));

        let turned = transform("rotate(90)") * kurbo::Point::new(1.0, 0.0);
        assert!((turned.x).abs() < 1e-9 && (turned.y - 1.0).abs() < 1e-9);

        let about = transform("rotate(180, 5, 5)") * kurbo::Point::ZERO;
        assert!((about.x - 10.0).abs() < 1e-9 && (about.y - 10.0).abs() < 1e-9);

        let matrix = transform("matrix(1 0 0 1 3 4)") * kurbo::Point::ZERO;
        assert_eq!(matrix, kurbo::Point::new(3.0, 4.0));
    }

    #[test]
    fn points_read_both_separators() {
        assert_eq!(points("0,0 10,0 10,10").len(), 3);
        assert_eq!(points("0 0 10 0 10 10").len(), 3);
        assert_eq!(points("0 0 10").len(), 1);
    }
}
