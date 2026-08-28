//! A path's own points: anchors and their control points.

use zgui::elements::kurbo::{self, PathEl};

use super::super::model::{SvgEdit, SvgModel, SvgTag};

/// Which point of a path element a handle stands on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Which {
    /// The element's end point, where the outline passes through.
    End,
    /// The first control point of a curve.
    Ctrl1,
    /// The second control point of a cubic.
    Ctrl2,
}

/// One draggable point of a path, in the path's own coordinates.
#[derive(Clone, Copy)]
pub struct NodePoint {
    /// Which element of the path it belongs to.
    pub element: usize,
    /// Which of its points it is.
    pub which: Which,
    /// Where it is.
    pub at: kurbo::Point,
}

impl NodePoint {
    /// Whether this is an anchor the outline passes through.
    #[must_use]
    pub fn is_anchor(&self) -> bool {
        self.which == Which::End
    }
}

/// Every draggable point of `path`, in order.
#[must_use]
pub fn points_of(path: &kurbo::BezPath) -> Vec<NodePoint> {
    let mut out = Vec::new();
    for (element, el) in path.elements().iter().enumerate() {
        let mut point = |which: Which, at: kurbo::Point| out.push(NodePoint { element, which, at });
        match *el {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => point(Which::End, p),
            PathEl::QuadTo(c, p) => {
                point(Which::Ctrl1, c);
                point(Which::End, p);
            }
            PathEl::CurveTo(c1, c2, p) => {
                point(Which::Ctrl1, c1);
                point(Which::Ctrl2, c2);
                point(Which::End, p);
            }
            PathEl::ClosePath => {}
        }
    }
    out
}

/// `path` with one point moved by `delta`, in the path's own coordinates.
#[must_use]
pub fn with_moved(
    path: &kurbo::BezPath,
    element: usize,
    which: Which,
    delta: kurbo::Vec2,
) -> kurbo::BezPath {
    let mut els = path.elements().to_vec();
    if let Some(el) = els.get_mut(element) {
        *el = match (*el, which) {
            (PathEl::MoveTo(p), Which::End) => PathEl::MoveTo(p + delta),
            (PathEl::LineTo(p), Which::End) => PathEl::LineTo(p + delta),
            (PathEl::QuadTo(c, p), Which::Ctrl1) => PathEl::QuadTo(c + delta, p),
            (PathEl::QuadTo(c, p), Which::End) => PathEl::QuadTo(c, p + delta),
            (PathEl::CurveTo(c1, c2, p), Which::Ctrl1) => PathEl::CurveTo(c1 + delta, c2, p),
            (PathEl::CurveTo(c1, c2, p), Which::Ctrl2) => PathEl::CurveTo(c1, c2 + delta, p),
            (PathEl::CurveTo(c1, c2, p), Which::End) => PathEl::CurveTo(c1, c2, p + delta),
            (other, _) => other,
        };
    }
    kurbo::BezPath::from_vec(els)
}

/// The edit that moves one point of one path by `delta` document units.
///
/// The whole `d` is rewritten from the moved outline, so its notation is normalised. Nothing
/// outside the one attribute changes.
#[must_use]
pub fn moved_point(
    model: &SvgModel,
    at: usize,
    element: usize,
    which: Which,
    delta: kurbo::Vec2,
) -> Option<SvgEdit> {
    let node = model.node(at)?;
    if node.tag != SvgTag::Path {
        return None;
    }
    let local = super::select::linear(node.to_doc.inverse(), delta);
    let moved = with_moved(&node.local, element, which, local);
    model.set_attr(at, "d", &moved.to_svg())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_curve_offers_its_anchors_and_controls() {
        let path = kurbo::BezPath::from_svg("M 0 0 C 1 2 3 4 5 6 L 7 8").expect("readable");
        let held = points_of(&path);
        assert_eq!(held.len(), 5);
        assert_eq!(held.iter().filter(|point| point.is_anchor()).count(), 3);
    }

    #[test]
    fn moving_an_anchor_rewrites_only_the_d() {
        let model = SvgModel::parse(
            r##"<svg xmlns="x" viewBox="0 0 10 10"><path d="M 0 0 L 4 0" fill="#abc"/></svg>"##,
            0,
        )
        .expect("readable");
        let edit = moved_point(&model, 0, 1, Which::End, kurbo::Vec2::new(0.0, 3.0))
            .expect("the path is there");
        let out = model.spliced(&edit);
        assert!(
            out.contains("L4 3") || out.contains("L 4 3") || out.contains("L4,3"),
            "{out}"
        );
        assert!(out.contains(r##"fill="#abc""##));
    }
}
