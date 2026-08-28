//! Cutting the drawing into the layers it is painted in.
//!
//! Shapes are painted into a canvas, but words and pictures cannot be: a canvas has no notion of
//! either. So the drawing is cut into bands — a run of shapes, then one word, then more shapes —
//! and each band becomes one element of the document. Their order is the drawing's order, so what
//! is drawn last is on top, whatever kind it is.
//!
//! A band is never split for any other reason: every extra canvas costs the renderer a pass.

use std::ops::Range;

use excalidraw::{Element, Kind};

/// One layer of the drawing.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Band {
    /// A run of shapes, painted into one canvas.
    Shapes(Range<usize>),
    /// One text element.
    Text(usize),
    /// One picture.
    Image(usize),
}

impl Band {
    /// Where in the drawing this band begins, which is what keeps it identified across a redraw.
    #[must_use]
    pub const fn at(&self) -> usize {
        match self {
            Self::Shapes(range) => range.start,
            Self::Text(at) | Self::Image(at) => *at,
        }
    }

    /// A word for what this band is, for a key that has to tell two apart.
    #[must_use]
    pub const fn kind(&self) -> u8 {
        match self {
            Self::Shapes(_) => 0,
            Self::Text(_) => 1,
            Self::Image(_) => 2,
        }
    }
}

/// The bands `elements` is painted in.
///
/// What is on screen has nothing to do with it: the bands depend only on the kinds and the order,
/// so moving the view rebuilds nothing and the renderer culls what it cannot see. A deleted element
/// paints nothing but does not break the run around it.
#[must_use]
pub fn of(elements: &[Element]) -> Vec<Band> {
    let mut bands = Vec::new();
    let mut run: Option<Range<usize>> = None;

    for (at, element) in elements.iter().enumerate() {
        let visible = !element.is_deleted;
        match element.kind {
            // A picture with no words and words with no picture: both are their own layer, and
            // both break the run of shapes around them.
            Kind::Text | Kind::Image if visible => {
                if let Some(range) = run.take() {
                    bands.push(Band::Shapes(range));
                }
                bands.push(if element.kind == Kind::Text {
                    Band::Text(at)
                } else {
                    Band::Image(at)
                });
            }
            // Words bound to a shape are drawn with it, not on their own.
            Kind::Text | Kind::Image => {}
            _ if visible => match &mut run {
                Some(range) => range.end = at + 1,
                None => run = Some(at..at + 1),
            },
            _ => {}
        }
    }
    if let Some(range) = run {
        bands.push(Band::Shapes(range));
    }
    bands
}

#[cfg(test)]
mod tests {
    use super::*;

    fn elements(kinds: &[&str]) -> Vec<Element> {
        kinds
            .iter()
            .enumerate()
            .map(|(at, kind)| {
                let json = format!(r#"{{"type":"{kind}","id":"e{at}","width":10,"height":10}}"#);
                let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
                excalidraw::element::read(value.as_object().expect("an object"))
                    .expect("an element")
            })
            .collect()
    }

    #[test]
    fn a_drawing_of_shapes_is_one_band() {
        let held = elements(&["rectangle", "ellipse", "arrow"]);
        assert_eq!(of(&held), [Band::Shapes(0..3)]);
    }

    #[test]
    fn words_cut_the_run_in_two() {
        let held = elements(&["rectangle", "text", "ellipse"]);
        assert_eq!(
            of(&held),
            [Band::Shapes(0..1), Band::Text(1), Band::Shapes(2..3)]
        );
    }

    #[test]
    fn a_picture_is_its_own_layer() {
        let held = elements(&["image", "rectangle"]);
        assert_eq!(of(&held), [Band::Image(0), Band::Shapes(1..2)]);
    }

    #[test]
    fn a_deleted_word_does_not_cut_the_run() {
        let json = r#"{"type":"text","id":"t","text":"hi","isDeleted":true}"#;
        let value: serde_json::Value = serde_json::from_str(json).expect("valid JSON");
        let gone =
            excalidraw::element::read(value.as_object().expect("an object")).expect("an element");
        let mut held = elements(&["rectangle", "ellipse"]);
        held.insert(1, gone);
        assert_eq!(of(&held), [Band::Shapes(0..3)]);
    }

    #[test]
    fn a_drawing_of_nothing_has_no_bands() {
        assert!(of(&[]).is_empty());
    }

    #[test]
    fn a_deleted_element_is_never_painted() {
        let json = r#"{"type":"rectangle","id":"a","isDeleted":true}"#;
        let value: serde_json::Value = serde_json::from_str(json).expect("valid JSON");
        let gone =
            excalidraw::element::read(value.as_object().expect("an object")).expect("an element");
        assert!(of(&[gone]).is_empty());
    }

    #[test]
    fn every_band_knows_where_it_begins() {
        let held = elements(&["rectangle", "text"]);
        let bands = of(&held);
        assert_eq!(bands[0].at(), 0);
        assert_eq!(bands[1].at(), 1);
        assert_ne!(bands[0].kind(), bands[1].kind());
    }
}
