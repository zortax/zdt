//! Which element a press lands on.

use zgui::elements::kurbo::{self, ParamCurveNearest as _, Shape as _};

use super::super::model::SvgModel;

/// The top-most node at `at`, in document coordinates.
///
/// Paint order decides: the last node drawn is the first asked. A filled inside catches the
/// press by winding; a stroke catches it within half its width, and `tolerance` widens both a
/// little so a hairline can be picked up at all.
#[must_use]
pub fn top_most(model: &SvgModel, at: kurbo::Point, tolerance: f64) -> Option<usize> {
    for (index, node) in model.nodes.iter().enumerate().rev() {
        let path = node.in_doc();
        if node.filled && path.winding(at) != 0 {
            return Some(index);
        }
        let reach = if node.stroke.as_deref().is_some_and(|held| held != "none") {
            node.stroke_width * scale_of(node.to_doc) / 2.0 + tolerance
        } else {
            tolerance
        };
        if distance(&path, at) <= reach {
            return Some(index);
        }
    }
    None
}

/// How far `at` is from the nearest point of `path`.
fn distance(path: &kurbo::BezPath, at: kurbo::Point) -> f64 {
    path.segments()
        .map(|segment| segment.nearest(at, 1e-3).distance_sq)
        .fold(f64::INFINITY, f64::min)
        .sqrt()
}

/// The affine's average magnification, for widths carried through it.
pub fn scale_of(affine: kurbo::Affine) -> f64 {
    let [a, b, c, d, ..] = affine.as_coeffs();
    (((a * a + b * b).sqrt() + (c * c + d * d).sqrt()) / 2.0).max(1e-9)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> SvgModel {
        SvgModel::parse(
            r##"<svg xmlns="x" viewBox="0 0 100 100">
  <rect x="10" y="10" width="40" height="40" fill="#f00"/>
  <rect x="30" y="30" width="40" height="40" fill="#0f0"/>
  <line x1="0" y1="90" x2="100" y2="90" stroke="#00f" stroke-width="4"/>
</svg>"##,
            0,
        )
        .expect("readable")
    }

    #[test]
    fn the_top_shape_wins_where_they_overlap() {
        let held = model();
        assert_eq!(top_most(&held, kurbo::Point::new(40.0, 40.0), 2.0), Some(1));
        assert_eq!(top_most(&held, kurbo::Point::new(15.0, 15.0), 2.0), Some(0));
        assert_eq!(top_most(&held, kurbo::Point::new(95.0, 15.0), 2.0), None);
    }

    #[test]
    fn a_stroke_catches_within_half_its_width() {
        let held = model();
        assert_eq!(top_most(&held, kurbo::Point::new(50.0, 91.5), 0.6), Some(2));
        assert_eq!(top_most(&held, kurbo::Point::new(50.0, 96.0), 0.6), None);
    }
}
