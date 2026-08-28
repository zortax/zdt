//! How large a run of words is.
//!
//! Nothing here asks the text engine: it estimates from the face's own metrics, which is enough to
//! give a text element a box that holds it and to decide where the lines break. What is drawn is
//! laid out by the engine, so the two agree closely and never exactly.

use excalidraw::text::{Font, Measure};
use excalidraw::{Element, Kind};

/// How wide one letter is, as a fraction of the font size.
///
/// A hand-drawn face is narrower than a fixed-width one, which is what these two say.
const PROPORTIONAL_ADVANCE: f64 = 0.52;
/// The same, for a fixed-width face.
const MONOSPACE_ADVANCE: f64 = 0.6;

/// A guess at how wide words are, from the face's own metrics.
pub struct Estimate;

impl Measure for Estimate {
    fn line_width(&self, text: &str, font: Font) -> f64 {
        let advance = if font.family.is_monospace() {
            MONOSPACE_ADVANCE
        } else {
            PROPORTIONAL_ADVANCE
        };
        // Counted in characters rather than bytes, so a line of Greek is not measured as twice its
        // length.
        text.chars().count() as f64 * font.size * advance
    }
}

/// How a run of words came out.
#[derive(Clone, PartialEq, Debug)]
pub struct Measured {
    /// What is drawn, with the breaks the wrapping put in.
    pub wrapped: String,
    /// How wide the widest line is.
    pub width: f64,
    /// How tall all of them are.
    pub height: f64,
}

/// How `typed` comes out in `element`.
///
/// `container` is the shape the words are written inside, when they are inside one: words in a
/// shape wrap to it, and free words grow instead.
#[must_use]
pub fn measure(element: &Element, container: Option<&Element>, typed: &str) -> Option<Measured> {
    if element.kind != Kind::Text {
        return None;
    }
    let words = element.text()?;
    let font = Font {
        family: words.font_family,
        size: words.font_size,
    };

    let wrapped = match container {
        Some(container) => {
            let limit = excalidraw::text::container_width(container, words.font_size);
            excalidraw::text::wrap(&Estimate, typed, font, limit)
        }
        // Free words wrap only where the reader put a break, unless they were dragged a width.
        None if words.auto_resize => typed.to_owned(),
        None => excalidraw::text::wrap(&Estimate, typed, font, element.width),
    };

    let width = excalidraw::text::width(&Estimate, &wrapped, font);
    let height = excalidraw::text::height(
        wrapped.split('\n').count(),
        words.font_size,
        words.line_height,
    );
    Some(Measured {
        wrapped,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use excalidraw::element::FontFamily;

    fn read(json: &str) -> Element {
        let value: serde_json::Value = serde_json::from_str(json).expect("valid JSON");
        excalidraw::element::read(value.as_object().expect("an object")).expect("an element")
    }

    #[test]
    fn free_words_keep_the_breaks_that_were_typed() {
        let held = read(r#"{"type":"text","id":"a","text":"","fontSize":20,"autoResize":true}"#);
        let out = measure(&held, None, "one two three").expect("a measure");
        assert_eq!(out.wrapped, "one two three");
        assert!(out.width > 0.0);
        assert!((out.height - 25.0).abs() < 1e-9);
    }

    #[test]
    fn words_in_a_shape_wrap_to_it() {
        let container = read(r#"{"type":"rectangle","id":"box","width":80,"height":100}"#);
        let held = read(
            r#"{"type":"text","id":"a","text":"","fontSize":20,"autoResize":false,
                "containerId":"box"}"#,
        );
        let out = measure(&held, Some(&container), "aaaa bbbb cccc").expect("a measure");
        assert!(out.wrapped.contains('\n'), "it wrapped: {:?}", out.wrapped);
        assert!(out.height > 25.0, "more than one line");
    }

    #[test]
    fn a_fixed_width_face_measures_wider() {
        let mono = Font {
            family: FontFamily::ComicShanns,
            size: 20.0,
        };
        let proportional = Font {
            family: FontFamily::Excalifont,
            size: 20.0,
        };
        assert!(Estimate.line_width("hello", mono) > Estimate.line_width("hello", proportional));
    }

    #[test]
    fn anything_that_is_not_words_is_not_measured() {
        let held = read(r#"{"type":"rectangle","id":"a"}"#);
        assert!(measure(&held, None, "hi").is_none());
    }

    #[test]
    fn a_line_is_measured_in_letters_not_bytes() {
        let font = Font {
            family: FontFamily::Excalifont,
            size: 20.0,
        };
        let latin = Estimate.line_width("abcde", font);
        let greek = Estimate.line_width("αβγδε", font);
        assert!((latin - greek).abs() < 1e-9);
    }
}
