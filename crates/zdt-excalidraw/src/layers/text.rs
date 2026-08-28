//! Drawing the words in a drawing.
//!
//! A canvas cannot draw words, so each text element is an ordinary label, placed and turned by the
//! view. That also gives it the text engine's own wrapping and its font fallback, and it is what
//! the in-place editor writes into.

use excalidraw::element::{Text, TextAlign, VerticalAlign};
use excalidraw::{Element, Kind};
use kurbo::Point;

use crate::viewport::Viewport;

/// Where a text element is drawn, in view pixels.
#[derive(Clone, PartialEq, Debug)]
pub struct Placed {
    /// Where its box begins.
    pub at: Point,
    /// How wide it is drawn.
    pub width: f64,
    /// How tall.
    pub height: f64,
    /// How tall the letters are drawn.
    pub font_size: f64,
    /// How far apart the lines are, as a multiple of that.
    pub line_height: f64,
    /// How far it is turned, in degrees.
    pub angle: f64,
    /// The words themselves.
    pub text: String,
    /// Where they sit across their box.
    pub align: TextAlign,
    /// And down it.
    pub vertical: VerticalAlign,
    /// Which family the host should draw them in.
    pub family: excalidraw::element::FontFamily,
    /// What they are drawn in.
    pub color: String,
    /// How solid they are.
    pub alpha: f64,
}

/// Where `element` is drawn, when it is words.
///
/// `container` is the shape the words are written inside, when they are inside one: bound words are
/// placed in the container's box rather than in their own.
#[must_use]
pub fn placed(
    element: &Element,
    container: Option<&Element>,
    viewport: &Viewport,
    dragged: Option<kurbo::Affine>,
) -> Option<Placed> {
    if element.kind != Kind::Text || element.is_deleted {
        return None;
    }
    let words: &Text = element.text()?;
    let zoom = viewport.zoom();

    // Bound words sit in the container's box and turn with it; free words are where they are.
    //
    // What is placed is the *middle* of the words, because the box is turned about its own middle
    // once it is on the page. Placing a corner and then turning about the middle would put a
    // turned label somewhere its container is not.
    let (width, height) = (element.width, element.height);
    let (middle, angle) = match container {
        Some(container) => {
            let box_ = excalidraw::text::container_box(container);
            let placement = excalidraw::geom::Placement::of(container);
            let inside = excalidraw::text::placed(
                box_,
                width,
                height,
                words.text_align,
                words.vertical_align,
            );
            let centre = Point::new(inside.x + width / 2.0, inside.y + height / 2.0);
            (placement.scene(centre), container.angle)
        }
        None => (
            Point::new(element.x + width / 2.0, element.y + height / 2.0),
            element.angle,
        ),
    };
    let scene_at = Point::new(middle.x - width / 2.0, middle.y - height / 2.0);

    // The drag moves them before it has been written, so they go with what they are drawn on. The
    // middle is what is moved, for the same reason it is what is placed.
    let scene_at = match dragged {
        Some(drag) => {
            let moved: Point = drag * middle;
            Point::new(moved.x - width / 2.0, moved.y - height / 2.0)
        }
        None => scene_at,
    };
    // Tracked, so words move with the view rather than staying where they were first drawn.
    let at = viewport.place(scene_at);
    Some(Placed {
        at,
        width: width * zoom,
        height: height * zoom,
        font_size: words.font_size * zoom,
        line_height: words.line_height,
        angle: angle.to_degrees(),
        text: words.text.clone(),
        align: words.text_align,
        vertical: words.vertical_align,
        family: words.font_family,
        color: element.stroke_color.clone(),
        alpha: element.alpha(),
    })
}

/// The shape `element`'s words are written inside, when they are inside one.
#[must_use]
pub fn container_of<'a>(element: &Element, elements: &'a [Element]) -> Option<&'a Element> {
    let id = element.text()?.container_id.as_ref()?;
    elements.iter().find(|held| &held.id == id)
}

/// The families the host should try, in order, for `family`.
///
/// The face's own name first, then whatever is installed that reads like it, so a drawing still
/// shows its words on a machine that has none of Excalidraw's faces.
#[must_use]
pub fn family_stack(family: excalidraw::element::FontFamily) -> String {
    let family = crate::fonts::drawn_as(family);
    let generic = if family.is_monospace() {
        "monospace"
    } else {
        "sans-serif"
    };
    format!("{}, Xiaolai, {generic}", family.name())
}

/// The word a horizontal alignment is written as in a style sheet.
#[must_use]
pub const fn align_word(align: TextAlign) -> &'static str {
    match align {
        TextAlign::Left => "left",
        TextAlign::Center => "center",
        TextAlign::Right => "right",
    }
}

/// The same, for a vertical one.
#[must_use]
pub const fn vertical_word(align: VerticalAlign) -> &'static str {
    match align {
        VerticalAlign::Top => "flex-start",
        VerticalAlign::Middle => "center",
        VerticalAlign::Bottom => "flex-end",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(json: &str) -> Element {
        let value: serde_json::Value = serde_json::from_str(json).expect("valid JSON");
        excalidraw::element::read(value.as_object().expect("an object")).expect("an element")
    }

    fn viewport() -> Viewport {
        let viewport = Viewport::new();
        viewport.set_size(800.0, 600.0);
        viewport
    }

    #[test]
    fn free_words_are_drawn_where_they_are() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let held = read(
                r#"{"type":"text","id":"a","x":100,"y":50,"width":80,"height":25,
                    "text":"hello","fontSize":20}"#,
            );
            let placed = placed(&held, None, &viewport(), None).expect("some words");
            assert!((placed.at.x - 100.0).abs() < 1e-9);
            assert_eq!(placed.text, "hello");
            assert!((placed.font_size - 20.0).abs() < 1e-9);
        });
    }

    #[test]
    fn the_zoom_reaches_the_size_the_letters_are_drawn_at() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let viewport = viewport();
            viewport.zoom_to(2.0);
            let held = read(r#"{"type":"text","id":"a","text":"hi","fontSize":20,"height":25}"#);
            let placed = placed(&held, None, &viewport, None).expect("some words");
            assert!((placed.font_size - 40.0).abs() < 1e-9);
        });
    }

    #[test]
    fn bound_words_sit_inside_their_container() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let container =
                read(r#"{"type":"rectangle","id":"box","x":100,"y":100,"width":200,"height":100}"#);
            let words = read(
                r#"{"type":"text","id":"t","x":0,"y":0,"width":100,"height":25,"text":"in",
                    "containerId":"box","textAlign":"center","verticalAlign":"middle"}"#,
            );
            let placed = placed(&words, Some(&container), &viewport(), None).expect("some words");
            // Centred in the box the container leaves for it.
            assert!(placed.at.x > 100.0 && placed.at.x < 300.0);
            assert!(placed.at.y > 100.0 && placed.at.y < 200.0);
        });
    }

    /// A turned label is placed so that turning it about its own middle puts it where its
    /// container is — which is what the style sheet then does.
    #[test]
    fn a_label_in_a_turned_shape_is_placed_by_its_middle() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let straight =
                read(r#"{"type":"rectangle","id":"box","x":100,"y":100,"width":200,"height":100}"#);
            let turned = read(
                r#"{"type":"rectangle","id":"box","x":100,"y":100,"width":200,"height":100,
                    "angle":1.5707963268}"#,
            );
            let words = read(
                r#"{"type":"text","id":"t","width":100,"height":25,"text":"in",
                    "containerId":"box","textAlign":"center","verticalAlign":"middle"}"#,
            );

            let middle = |placed: &Placed| {
                Point::new(
                    placed.at.x + placed.width / 2.0,
                    placed.at.y + placed.height / 2.0,
                )
            };
            let before = placed(&words, Some(&straight), &viewport(), None).expect("words");
            let after = placed(&words, Some(&turned), &viewport(), None).expect("words");
            // Centred in a box, the middle is the same before and after a quarter turn.
            assert!(
                (middle(&before) - middle(&after)).hypot() < 1.0,
                "{:?} against {:?}",
                middle(&before),
                middle(&after)
            );
            assert!(
                (after.angle - 90.0).abs() < 1e-6,
                "and it is turned with it"
            );
        });
    }

    #[test]
    fn anything_that_is_not_words_is_not_placed() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let held = read(r#"{"type":"rectangle","id":"a"}"#);
            assert!(placed(&held, None, &viewport(), None).is_none());
            let gone = read(r#"{"type":"text","id":"b","text":"hi","isDeleted":true}"#);
            assert!(placed(&gone, None, &viewport(), None).is_none());
        });
    }

    #[test]
    fn a_container_is_found_by_the_name_the_words_carry() {
        let elements = vec![
            read(r#"{"type":"rectangle","id":"box"}"#),
            read(r#"{"type":"text","id":"t","text":"in","containerId":"box"}"#),
        ];
        let found = container_of(&elements[1], &elements).expect("the container");
        assert_eq!(found.id.as_str(), "box");
        assert!(container_of(&elements[0], &elements).is_none());
    }

    #[test]
    fn a_monospace_face_asks_for_a_monospace_fallback() {
        use excalidraw::element::FontFamily;
        assert!(family_stack(FontFamily::ComicShanns).ends_with("monospace"));
        assert!(family_stack(FontFamily::Excalifont).ends_with("sans-serif"));
        assert!(family_stack(FontFamily::Excalifont).starts_with("Excalifont"));
    }
}
