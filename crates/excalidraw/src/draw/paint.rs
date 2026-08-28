//! One shape, and what paints it.

use std::sync::Arc;

use excalidraw_rough::ops::{Drawable, OpSetKind};
use excalidraw_rough::to_path;
use kurbo::{BezPath, Stroke};

use crate::element::{Arrowhead, Element};
use crate::geom::arrowhead;

/// What one shape is painted with.
#[derive(Clone, PartialEq, Debug)]
pub struct Paint {
    /// The colour, as the element wrote it.
    pub color: String,
    /// How solid it is, from nothing to one.
    pub alpha: f64,
}

/// One shape of a drawn element.
#[derive(Clone, PartialEq, Debug)]
pub struct Piece {
    /// The outline. Shared, so an element that has not changed hands back the same allocation.
    pub path: Arc<BezPath>,
    /// What fills it, when anything does.
    pub fill: Option<Paint>,
    /// What strokes it, and how wide.
    pub stroke: Option<(Paint, Stroke)>,
    /// Whether the inside is decided by the even-odd rule.
    pub even_odd: bool,
}

impl Piece {
    /// Whether it paints nothing at all.
    #[must_use]
    pub fn is_blank(&self) -> bool {
        self.fill.is_none() && self.stroke.is_none()
    }
}

/// The pieces `drawn` is, painted with `element`'s colours.
#[must_use]
pub fn of_drawable(drawn: &Drawable, element: &Element) -> Vec<Piece> {
    let alpha = element.alpha();
    let stroke_style = super::stroke(element);
    let mut pieces = Vec::with_capacity(drawn.sets.len());

    for set in &drawn.sets {
        let path = Arc::new(to_path::of_set(set));
        if path.is_empty() {
            continue;
        }
        let piece = match set.kind {
            // A solid fill is a ring the background colour fills. The even-odd rule is what keeps
            // the hole in a shape drawn as one continuous path.
            OpSetKind::FillPath => Piece {
                path,
                fill: super::fill_color(element).map(|color| Paint {
                    color: color.to_owned(),
                    alpha,
                }),
                stroke: None,
                even_odd: true,
            },
            // A patterned fill is strokes, drawn in the background colour at the fill weight.
            OpSetKind::FillSketch => Piece {
                path,
                fill: None,
                stroke: super::fill_color(element).map(|color| {
                    (
                        Paint {
                            color: color.to_owned(),
                            alpha,
                        },
                        Stroke::new(fill_weight(element))
                            .with_caps(kurbo::Cap::Round)
                            .with_join(kurbo::Join::Round),
                    )
                }),
                even_odd: false,
            },
            OpSetKind::Path => Piece {
                path,
                fill: None,
                stroke: Some((
                    Paint {
                        color: element.stroke_color.clone(),
                        alpha,
                    },
                    stroke_style.clone(),
                )),
                even_odd: false,
            },
        };
        if !piece.is_blank() {
            pieces.push(piece);
        }
    }
    pieces
}

/// How wide one stroke of a patterned fill is.
fn fill_weight(element: &Element) -> f64 {
    (element.stroke_width / 2.0).max(0.1)
}

/// The pieces one arrowhead is.
///
/// An open head is filled with the page's colour rather than the line's, so the line behind it does
/// not show through. The caller has the page colour, so an open head asks for no fill and is filled
/// by whoever knows.
#[must_use]
pub fn of_arrowhead(head: Arrowhead, at: &arrowhead::Geometry, element: &Element) -> Vec<Piece> {
    let alpha = element.alpha();
    let drawn = arrowhead::draw(head, at);
    // A head is always solid, however the line is broken up.
    let stroke = Stroke::new(element.stroke_width)
        .with_caps(kurbo::Cap::Round)
        .with_join(kurbo::Join::Round);

    let mut pieces: Vec<Piece> = drawn
        .strokes
        .into_iter()
        .map(|path| Piece {
            path: Arc::new(path),
            fill: None,
            stroke: Some((
                Paint {
                    color: element.stroke_color.clone(),
                    alpha,
                },
                stroke.clone(),
            )),
            even_odd: false,
        })
        .collect();

    if let Some(path) = drawn.filled {
        pieces.push(Piece {
            path: Arc::new(path),
            fill: drawn.fill_is_stroke.then(|| Paint {
                color: element.stroke_color.clone(),
                alpha,
            }),
            stroke: Some((
                Paint {
                    color: element.stroke_color.clone(),
                    alpha,
                },
                stroke,
            )),
            even_odd: false,
        });
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(json: &str) -> Element {
        let value: serde_json::Value = serde_json::from_str(json).expect("valid JSON");
        crate::element::read(value.as_object().expect("an object")).expect("an element")
    }

    #[test]
    fn an_unfilled_shape_is_one_stroked_piece() {
        let held = read(r#"{"type":"rectangle","width":100,"height":50,"seed":1}"#);
        let pieces = super::super::pieces(&held);
        assert_eq!(pieces.len(), 1);
        assert!(pieces[0].fill.is_none());
        assert!(pieces[0].stroke.is_some());
    }

    #[test]
    fn a_filled_shape_paints_its_inside_first() {
        let held = read(
            r##"{"type":"rectangle","width":100,"height":50,"seed":1,
                 "backgroundColor":"#a5d8ff","fillStyle":"hachure"}"##,
        );
        let pieces = super::super::pieces(&held);
        assert_eq!(pieces.len(), 2);
        assert!(
            pieces[0].stroke.as_ref().expect("the fill strokes").0.color == "#a5d8ff",
            "the inside is painted in the background colour"
        );
    }

    #[test]
    fn opacity_reaches_every_piece() {
        let held = read(
            r##"{"type":"rectangle","width":100,"height":50,"seed":1,
                 "backgroundColor":"#a5d8ff","opacity":40}"##,
        );
        for piece in super::super::pieces(&held) {
            let alpha = piece
                .fill
                .as_ref()
                .map(|paint| paint.alpha)
                .or_else(|| piece.stroke.as_ref().map(|held| held.0.alpha))
                .expect("something paints it");
            assert!((alpha - 0.4).abs() < 1e-9);
        }
    }

    #[test]
    fn a_dashed_outline_carries_its_dashes() {
        let held =
            read(r#"{"type":"rectangle","width":100,"height":50,"seed":1,"strokeStyle":"dashed"}"#);
        let pieces = super::super::pieces(&held);
        let (_, stroke) = pieces[0].stroke.as_ref().expect("it strokes");
        assert!(!stroke.dash_pattern.is_empty());
    }

    #[test]
    fn a_pen_stroke_is_a_filled_ring() {
        let held = read(
            r##"{"type":"freedraw","points":[[0,0],[20,4],[40,0]],"seed":1,
                "simulatePressure":true,"strokeColor":"#1971c2"}"##,
        );
        let pieces = super::super::pieces(&held);
        assert_eq!(pieces.len(), 1);
        assert_eq!(
            pieces[0].fill.as_ref().expect("it fills").color,
            "#1971c2",
            "a pen stroke is filled in the colour of its line"
        );
        assert!(pieces[0].stroke.is_none());
    }

    #[test]
    fn an_arrow_carries_its_head() {
        let bare =
            read(r#"{"type":"arrow","points":[[0,0],[100,0]],"seed":1,"endArrowhead":null}"#);
        let headed = read(r#"{"type":"arrow","points":[[0,0],[100,0]],"seed":1}"#);
        assert!(
            super::super::pieces(&headed).len() > super::super::pieces(&bare).len(),
            "the head is drawn on top of the line"
        );
    }

    #[test]
    fn a_deleted_element_draws_nothing() {
        let held =
            read(r#"{"type":"rectangle","width":100,"height":50,"seed":1,"isDeleted":true}"#);
        assert!(super::super::pieces(&held).is_empty());
    }

    #[test]
    fn a_turned_element_is_drawn_where_it_is_turned_to() {
        use kurbo::Shape as _;
        let held = read(
            r#"{"type":"rectangle","x":0,"y":0,"width":100,"height":20,"seed":1,
                "angle":1.5707963268}"#,
        );
        let bounds = super::super::pieces(&held)[0].path.bounding_box();
        assert!(bounds.height() > bounds.width(), "it stands on its end");
    }
}
