//! Painting a run of shapes.
//!
//! The canvas is given the visible square of the scene as its view box and the shapes in the
//! scene's own coordinates. So a pan or a zoom writes one property and re-runs no drawing: the
//! paths are the same allocations, and the rasteriser keeps what it made of them.

use excalidraw::draw::{Cache, Piece};
use excalidraw::{Element, Kind};
use zgui::canvas::{Brush, ShapeBuilder};

use crate::color;

/// Every piece the elements in `range` are painted as, in order.
///
/// `dragged` is what a drag under way is doing, and `carried` says which elements it is doing it
/// to. Those are painted where the drag has taken them rather than where they are written, so a
/// change shows before it has been made — and it shows in this band, so what was drawn over it
/// still is.
///
/// Shapes come from `cache`, so an element that has not changed keeps the paths it already had.
/// That is what a band redraw costs almost nothing: the same allocations go back to the
/// rasteriser, which recognises them and keeps what it made of them.
#[must_use]
pub fn pieces(
    cache: &mut Cache,
    elements: &[Element],
    range: std::ops::Range<usize>,
    dragged: Option<kurbo::Affine>,
    carried: impl Fn(&excalidraw::Id) -> bool,
    fade: impl Fn(&excalidraw::Id) -> f64,
) -> Vec<Piece> {
    elements
        .get(range)
        .unwrap_or_default()
        .iter()
        .filter(|element| !matches!(element.kind, Kind::Text | Kind::Image))
        .flat_map(|element| {
            let drawn = cache.pieces(element);
            let faded = fade(&element.id);
            let held = dragged.filter(|_| carried(&element.id));
            // Nothing to do to them is the usual case, and it hands the shapes on as they are.
            if held.is_none() && (faded - 1.0).abs() < f64::EPSILON {
                return drawn.as_slice().to_vec();
            }
            drawn
                .iter()
                .map(|piece| {
                    let piece = match held {
                        Some(drag) => moved(piece.clone(), drag),
                        None => piece.clone(),
                    };
                    if (faded - 1.0).abs() < f64::EPSILON {
                        piece
                    } else {
                        dimmed(piece, faded)
                    }
                })
                .collect()
        })
        .collect()
}

/// One piece, drawn at `by` of its own solidity.
fn dimmed(piece: Piece, by: f64) -> Piece {
    let paint = |held: excalidraw::draw::Paint| excalidraw::draw::Paint {
        alpha: held.alpha * by,
        ..held
    };
    Piece {
        fill: piece.fill.map(paint),
        stroke: piece.stroke.map(|(held, stroke)| (paint(held), stroke)),
        ..piece
    }
}

/// One piece, taken where the drag has taken it.
fn moved(piece: Piece, drag: kurbo::Affine) -> Piece {
    let mut path = kurbo::BezPath::clone(&piece.path);
    path.apply_affine(drag);
    Piece {
        path: std::sync::Arc::new(path),
        ..piece
    }
}

/// Puts `pieces` into `scene`.
pub fn push(scene: &mut zgui::canvas::CanvasScene, pieces: &[Piece], dark: bool) {
    for piece in pieces {
        let mut shape = ShapeBuilder::shared(std::sync::Arc::clone(&piece.path));
        if let Some(fill) = &piece.fill {
            let brush = Brush::Solid(color::in_scheme(&fill.color, fill.alpha, dark));
            shape = if piece.even_odd {
                shape.fill_even_odd(brush)
            } else {
                shape.fill(brush)
            };
        } else if piece.stroke.is_none() {
            continue;
        }
        if let Some((paint, stroke)) = &piece.stroke {
            shape = shape.stroke_styled(
                Brush::Solid(color::in_scheme(&paint.color, paint.alpha, dark)),
                stroke.clone(),
            );
        }
        scene.push(shape.build());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(json: &str) -> Element {
        let value: serde_json::Value = serde_json::from_str(json).expect("valid JSON");
        excalidraw::element::read(value.as_object().expect("an object")).expect("an element")
    }

    #[test]
    fn words_and_pictures_are_not_painted_into_the_canvas() {
        let held = vec![
            read(r#"{"type":"rectangle","id":"a","width":10,"height":10,"seed":1}"#),
            read(r#"{"type":"text","id":"b","text":"hi"}"#),
            read(r#"{"type":"image","id":"c","width":10,"height":10}"#),
        ];
        assert!(!pieces(&mut Cache::new(), &held, 0..3, None, |_| false, |_| 1.0).is_empty());
        assert!(pieces(&mut Cache::new(), &held, 1..3, None, |_| false, |_| 1.0).is_empty());
    }

    #[test]
    fn what_is_being_dragged_is_painted_where_the_drag_has_taken_it() {
        use kurbo::Shape as _;

        let held = vec![read(
            r#"{"type":"rectangle","id":"a","x":0,"y":0,"width":10,"height":10,"seed":1}"#,
        )];
        let still = pieces(&mut Cache::new(), &held, 0..1, None, |_| false, |_| 1.0);
        let moved = pieces(
            &mut Cache::new(),
            &held,
            0..1,
            Some(kurbo::Affine::translate((100.0, 0.0))),
            |_| true,
            |_| 1.0,
        );
        assert_eq!(still.len(), moved.len(), "the same shape, in another place");
        let x = |pieces: &[Piece]| pieces[0].path.bounding_box().x0;
        assert!((x(&moved) - x(&still) - 100.0).abs() < 1e-6);

        // And what the drag is not holding stays where it is.
        let untouched = pieces(
            &mut Cache::new(),
            &held,
            0..1,
            Some(kurbo::Affine::translate((100.0, 0.0))),
            |_| false,
            |_| 1.0,
        );
        assert!((x(&untouched) - x(&still)).abs() < 1e-9);
    }

    #[test]
    fn what_the_eraser_has_marked_is_drawn_faintly() {
        let held = vec![read(
            r##"{"type":"rectangle","id":"a","width":100,"height":50,"seed":1,
                 "backgroundColor":"#a5d8ff"}"##,
        )];
        let solid = pieces(&mut Cache::new(), &held, 0..1, None, |_| false, |_| 1.0);
        let faint = pieces(&mut Cache::new(), &held, 0..1, None, |_| false, |_| 0.2);
        assert_eq!(solid.len(), faint.len(), "the same shape, more faintly");

        let alpha = |pieces: &[Piece]| {
            pieces
                .iter()
                .filter_map(|piece| {
                    piece
                        .fill
                        .as_ref()
                        .map(|paint| paint.alpha)
                        .or_else(|| piece.stroke.as_ref().map(|held| held.0.alpha))
                })
                .fold(0.0_f64, f64::max)
        };
        assert!(alpha(&faint) < alpha(&solid) * 0.5, "and much more faintly");
    }

    #[test]
    fn a_range_past_the_end_paints_nothing() {
        let held = vec![read(
            r#"{"type":"rectangle","id":"a","width":10,"height":10}"#,
        )];
        assert!(pieces(&mut Cache::new(), &held, 5..9, None, |_| false, |_| 1.0).is_empty());
    }

    #[test]
    fn every_piece_reaches_the_scene() {
        let held = vec![read(
            r##"{"type":"rectangle","id":"a","width":100,"height":50,"seed":1,
                 "backgroundColor":"#a5d8ff"}"##,
        )];
        let drawn = pieces(&mut Cache::new(), &held, 0..1, None, |_| false, |_| 1.0);
        let mut scene = zgui::canvas::CanvasScene::default();
        push(&mut scene, &drawn, false);
        assert_eq!(scene.shapes().len(), drawn.len());
        assert!(scene.shapes().len() >= 2, "an inside and an outline");
    }
}
