//! Moving and scaling whole elements.

use zgui::elements::kurbo;

use super::super::model::geometry::{attr, number, points};
use super::super::model::write::{fmt, points_attr, transform_attr};
use super::super::model::{SvgEdit, SvgModel, SvgTag};
use super::Sides;

/// A vector carried through the linear part of `affine` alone.
pub fn linear(affine: kurbo::Affine, vector: kurbo::Vec2) -> kurbo::Vec2 {
    let [a, b, c, d, ..] = affine.as_coeffs();
    kurbo::Vec2::new(a * vector.x + c * vector.y, b * vector.x + d * vector.y)
}

/// The edit that moves one node by `delta` document units.
///
/// A shape with place attributes moves through them, so the source diff stays readable. A path
/// moves through its `transform`, because rewriting every coordinate of `d` for a drag would be
/// the larger diff.
#[must_use]
pub fn moved(model: &SvgModel, at: usize, delta: kurbo::Vec2) -> Option<SvgEdit> {
    let node = model.node(at)?;
    let local = linear(node.to_doc.inverse(), delta);
    if local.x.abs() < 1e-9 && local.y.abs() < 1e-9 {
        return None;
    }
    let shifted = |name: &'static str, by: f64| -> (&'static str, String) {
        (name, fmt(number(&node.attrs, name, 0.0) + by))
    };
    match node.tag {
        SvgTag::Rect => model.set_attrs(at, &[shifted("x", local.x), shifted("y", local.y)]),
        SvgTag::Circle | SvgTag::Ellipse => {
            model.set_attrs(at, &[shifted("cx", local.x), shifted("cy", local.y)])
        }
        SvgTag::Line => model.set_attrs(
            at,
            &[
                shifted("x1", local.x),
                shifted("y1", local.y),
                shifted("x2", local.x),
                shifted("y2", local.y),
            ],
        ),
        SvgTag::Polyline | SvgTag::Polygon => {
            let mut held = points(attr(&node.attrs, "points")?);
            for point in &mut held {
                *point += local;
            }
            model.set_attr(at, "points", &points_attr(&held))
        }
        SvgTag::Path => {
            let parent = node.to_doc * node.own.inverse();
            let step = linear(parent.inverse(), delta);
            let own = kurbo::Affine::translate(step) * node.own;
            model.set_attr(at, "transform", &transform_attr(own))
        }
    }
}

/// The edit that scales one node, dragging the sides `sides` by `delta` document units.
///
/// The smallest result is kept above nothing: a box scaled through itself flips no sign and
/// deletes no shape.
#[must_use]
pub fn scaled(model: &SvgModel, at: usize, sides: Sides, delta: kurbo::Vec2) -> Option<SvgEdit> {
    /// The narrowest a scaled box may get, in document units.
    const SLIGHTEST: f64 = 0.5;

    let node = model.node(at)?;
    let bounds = node.bounds();
    if bounds.width() <= 0.0 || bounds.height() <= 0.0 {
        return None;
    }

    let mut next = bounds;
    if sides.left {
        next.x0 = (bounds.x0 + delta.x).min(bounds.x1 - SLIGHTEST);
    }
    if sides.right {
        next.x1 = (bounds.x1 + delta.x).max(bounds.x0 + SLIGHTEST);
    }
    if sides.top {
        next.y0 = (bounds.y0 + delta.y).min(bounds.y1 - SLIGHTEST);
    }
    if sides.bottom {
        next.y1 = (bounds.y1 + delta.y).max(bounds.y0 + SLIGHTEST);
    }

    let (sx, sy) = (
        next.width() / bounds.width(),
        next.height() / bounds.height(),
    );
    let anchor = kurbo::Point::new(
        if sides.left { bounds.x1 } else { bounds.x0 },
        if sides.top { bounds.y1 } else { bounds.y0 },
    );

    // A plain rectangle in the document's own space keeps its place attributes readable.
    if node.tag == SvgTag::Rect && node.to_doc == kurbo::Affine::IDENTITY {
        return model.set_attrs(
            at,
            &[
                ("x", fmt(next.x0)),
                ("y", fmt(next.y0)),
                ("width", fmt(next.width())),
                ("height", fmt(next.height())),
            ],
        );
    }

    let scale = kurbo::Affine::translate(anchor.to_vec2())
        * kurbo::Affine::scale_non_uniform(sx, sy)
        * kurbo::Affine::translate(-anchor.to_vec2());
    let parent = node.to_doc * node.own.inverse();
    let own = parent.inverse() * scale * parent * node.own;
    model.set_attr(at, "transform", &transform_attr(own))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> SvgModel {
        SvgModel::parse(
            r##"<svg xmlns="x" viewBox="0 0 100 100">
  <rect x="10" y="10" width="20" height="20" fill="#f00"/>
  <g transform="translate(50 0) scale(2)">
    <circle cx="5" cy="5" r="2"/>
  </g>
  <path d="M 0 0 L 10 10"/>
  <polygon points="0,0 10,0 5,8"/>
</svg>"##,
            3,
        )
        .expect("readable")
    }

    fn applied(model: &SvgModel, edit: SvgEdit) -> SvgModel {
        SvgModel::parse(&model.spliced(&edit), model.revision + 1).expect("still readable")
    }

    #[test]
    fn a_rect_moves_through_its_place() {
        let held = model();
        let edit = moved(&held, 0, kurbo::Vec2::new(5.0, -2.5)).expect("it moves");
        let out = applied(&held, edit);
        assert_eq!(number(&out.nodes[0].attrs, "x", 0.0), 15.0);
        assert_eq!(number(&out.nodes[0].attrs, "y", 0.0), 7.5);
    }

    #[test]
    fn a_move_lands_in_the_ancestors_space() {
        let held = model();
        // The group doubles everything, so ten document units are five local ones.
        let edit = moved(&held, 1, kurbo::Vec2::new(10.0, 0.0)).expect("it moves");
        let out = applied(&held, edit);
        assert_eq!(number(&out.nodes[1].attrs, "cx", 0.0), 10.0);
        assert_eq!(number(&out.nodes[1].attrs, "cy", 0.0), 5.0);
    }

    #[test]
    fn a_path_moves_through_its_transform() {
        let held = model();
        let edit = moved(&held, 2, kurbo::Vec2::new(3.0, 4.0)).expect("it moves");
        let out = held.spliced(&edit);
        assert!(out.contains(r#"transform="translate(3 4)""#));
        // The notation is untouched.
        assert!(out.contains(r#"d="M 0 0 L 10 10""#));
    }

    #[test]
    fn a_polygon_moves_every_point() {
        let held = model();
        let edit = moved(&held, 3, kurbo::Vec2::new(1.0, 1.0)).expect("it moves");
        let out = held.spliced(&edit);
        assert!(out.contains(r#"points="1,1 11,1 6,9""#));
    }

    #[test]
    fn a_plain_rect_scales_through_its_box() {
        let held = model();
        let sides = Sides {
            right: true,
            bottom: true,
            ..Sides::default()
        };
        let edit = scaled(&held, 0, sides, kurbo::Vec2::new(10.0, 5.0)).expect("it scales");
        let out = applied(&held, edit);
        assert_eq!(number(&out.nodes[0].attrs, "width", 0.0), 30.0);
        assert_eq!(number(&out.nodes[0].attrs, "height", 0.0), 25.0);
        assert_eq!(number(&out.nodes[0].attrs, "x", 0.0), 10.0);
    }

    #[test]
    fn a_transformed_shape_scales_through_its_transform_and_holds_its_anchor() {
        let held = model();
        let before = held.nodes[1].bounds();
        let sides = Sides {
            right: true,
            bottom: true,
            ..Sides::default()
        };
        let edit = scaled(&held, 1, sides, kurbo::Vec2::new(4.0, 4.0)).expect("it scales");
        let out = applied(&held, edit);
        let after = out.nodes[1].bounds();
        // The dragged corner grew by the delta; the anchored corner stayed.
        assert!((after.x1 - (before.x1 + 4.0)).abs() < 0.05);
        assert!((after.x0 - before.x0).abs() < 0.05);
        assert!((after.y0 - before.y0).abs() < 0.05);
    }

    #[test]
    fn a_scale_cannot_take_a_box_through_itself() {
        let held = model();
        let sides = Sides {
            right: true,
            ..Sides::default()
        };
        let edit = scaled(&held, 0, sides, kurbo::Vec2::new(-500.0, 0.0)).expect("it scales");
        let out = applied(&held, edit);
        assert!(number(&out.nodes[0].attrs, "width", 0.0) > 0.0);
    }
}
