//! Making a new element.
//!
//! The keys are written in the order Excalidraw writes them, so a drawing this crate adds to reads
//! the same as one the web app added to, and a diff between two saves shows only what changed.

use kurbo::Point;
use serde_json::{Map, Value};

use crate::element::style::{
    Arrowhead, FillStyle, Roundness, StrokeStyle, TextAlign, VerticalAlign,
};
use crate::element::{FontFamily, Id, Kind, Seed};
use crate::store::Number;

/// What every new element is given.
#[derive(Clone, PartialEq, Debug)]
pub struct Style {
    /// What its outline is drawn in.
    pub stroke_color: String,
    /// What its inside is filled with.
    pub background_color: String,
    /// How that inside is drawn.
    pub fill_style: FillStyle,
    /// How wide the outline is.
    pub stroke_width: f64,
    /// How it is broken up.
    pub stroke_style: StrokeStyle,
    /// How far the hand wanders.
    pub roughness: f64,
    /// How solid it is.
    pub opacity: f64,
    /// How its corners are cut, when they are.
    pub roundness: bool,
    /// How tall its letters are.
    pub font_size: f64,
    /// Which face they are in.
    pub font_family: FontFamily,
    /// Where they sit across their box.
    pub text_align: TextAlign,
    /// And down it.
    pub vertical_align: VerticalAlign,
    /// What decorates the start of a new arrow.
    pub start_arrowhead: Option<Arrowhead>,
    /// And its end.
    pub end_arrowhead: Option<Arrowhead>,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            stroke_color: crate::element::read::DEFAULT_STROKE_COLOR.to_owned(),
            background_color: crate::element::read::DEFAULT_BACKGROUND_COLOR.to_owned(),
            fill_style: FillStyle::Solid,
            stroke_width: crate::element::read::DEFAULT_STROKE_WIDTH,
            stroke_style: StrokeStyle::Solid,
            roughness: crate::element::read::DEFAULT_ROUGHNESS,
            opacity: crate::element::read::DEFAULT_OPACITY,
            roundness: true,
            font_size: crate::element::font::DEFAULT_FONT_SIZE,
            font_family: FontFamily::Excalifont,
            text_align: TextAlign::Left,
            vertical_align: VerticalAlign::Top,
            start_arrowhead: None,
            end_arrowhead: Some(Arrowhead::Arrow),
        }
    }
}

/// The box a new element is drawn in.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Box_ {
    /// Where its unrotated box begins.
    pub x: f64,
    /// The same, down the page.
    pub y: f64,
    /// How wide it is.
    pub width: f64,
    /// How tall.
    pub height: f64,
}

/// The word for a rounding, as the file writes it.
fn roundness_json(kind: Kind, wanted: bool) -> Value {
    if !wanted || !kind.can_be_round() {
        return Value::Null;
    }
    let mut object = Map::new();
    let which = if kind.adaptive_radius() { 3 } else { 2 };
    object.insert("type".to_owned(), Value::from(which));
    Value::Object(object)
}

/// The head, as the file writes it.
fn arrowhead_json(head: Option<Arrowhead>) -> Value {
    head.map_or(Value::Null, |head| Value::String(head.as_str().to_owned()))
}

/// A new element of `kind`, in the box and style given.
///
/// `id`, `seed` and `version_nonce` are the caller's, so a session that has to be repeatable can
/// hand in a generator it controls.
#[must_use]
pub fn element(
    kind: Kind,
    box_: Box_,
    style: &Style,
    id: &Id,
    seed: Seed,
    version_nonce: u64,
    updated: u64,
) -> Value {
    let mut object = Map::new();
    object.insert("id".to_owned(), Value::String(id.as_str().to_owned()));
    object.insert("type".to_owned(), Value::String(kind.as_str().to_owned()));
    object.insert("x".to_owned(), Number::json(box_.x));
    object.insert("y".to_owned(), Number::json(box_.y));
    object.insert("width".to_owned(), Number::json(box_.width));
    object.insert("height".to_owned(), Number::json(box_.height));
    object.insert("angle".to_owned(), Number::json(0.0));
    object.insert(
        "strokeColor".to_owned(),
        Value::String(style.stroke_color.clone()),
    );
    object.insert(
        "backgroundColor".to_owned(),
        Value::String(style.background_color.clone()),
    );
    object.insert("fillStyle".to_owned(), word(style.fill_style));
    object.insert("strokeWidth".to_owned(), Number::json(style.stroke_width));
    object.insert("strokeStyle".to_owned(), word(style.stroke_style));
    object.insert("roughness".to_owned(), Number::json(style.roughness));
    object.insert("opacity".to_owned(), Number::json(style.opacity));
    object.insert("groupIds".to_owned(), Value::Array(Vec::new()));
    object.insert("frameId".to_owned(), Value::Null);
    object.insert("index".to_owned(), Value::Null);
    object.insert(
        "roundness".to_owned(),
        roundness_json(kind, style.roundness),
    );
    object.insert("seed".to_owned(), Value::from(seed.0));
    object.insert("version".to_owned(), Value::from(1));
    object.insert("versionNonce".to_owned(), Value::from(version_nonce));
    object.insert("isDeleted".to_owned(), Value::Bool(false));
    object.insert("boundElements".to_owned(), Value::Null);
    object.insert("updated".to_owned(), Value::from(updated));
    object.insert("link".to_owned(), Value::Null);
    object.insert("locked".to_owned(), Value::Bool(false));

    match kind {
        Kind::Line | Kind::Arrow => {
            object.insert("points".to_owned(), points_json(&[Point::ZERO]));
            object.insert("lastCommittedPoint".to_owned(), Value::Null);
            object.insert("startBinding".to_owned(), Value::Null);
            object.insert("endBinding".to_owned(), Value::Null);
            object.insert(
                "startArrowhead".to_owned(),
                arrowhead_json(if kind == Kind::Arrow {
                    style.start_arrowhead
                } else {
                    None
                }),
            );
            object.insert(
                "endArrowhead".to_owned(),
                arrowhead_json(if kind == Kind::Arrow {
                    style.end_arrowhead
                } else {
                    None
                }),
            );
            if kind == Kind::Arrow {
                object.insert("elbowed".to_owned(), Value::Bool(false));
            } else {
                object.insert("polygon".to_owned(), Value::Bool(false));
            }
        }
        Kind::Freedraw => {
            object.insert("points".to_owned(), points_json(&[Point::ZERO]));
            object.insert("pressures".to_owned(), Value::Array(Vec::new()));
            object.insert("simulatePressure".to_owned(), Value::Bool(true));
            let mut options = Map::new();
            options.insert(
                "variability".to_owned(),
                Value::String("variable".to_owned()),
            );
            options.insert(
                "streamline".to_owned(),
                Number::json(crate::element::read::DEFAULT_STREAMLINE),
            );
            object.insert("strokeOptions".to_owned(), Value::Object(options));
        }
        Kind::Text => {
            object.insert("fontSize".to_owned(), Number::json(style.font_size));
            object.insert(
                "fontFamily".to_owned(),
                Value::from(style.font_family.to_number()),
            );
            object.insert("text".to_owned(), Value::String(String::new()));
            object.insert("textAlign".to_owned(), word(style.text_align));
            object.insert("verticalAlign".to_owned(), word(style.vertical_align));
            object.insert("containerId".to_owned(), Value::Null);
            object.insert("originalText".to_owned(), Value::String(String::new()));
            object.insert("autoResize".to_owned(), Value::Bool(true));
            object.insert(
                "lineHeight".to_owned(),
                Number::json(style.font_family.line_height()),
            );
        }
        Kind::Image => {
            object.insert("fileId".to_owned(), Value::Null);
            object.insert("status".to_owned(), Value::String("pending".to_owned()));
            object.insert(
                "scale".to_owned(),
                Value::Array(vec![Value::from(1), Value::from(1)]),
            );
            object.insert("crop".to_owned(), Value::Null);
        }
        Kind::Frame | Kind::Magicframe => {
            object.insert("name".to_owned(), Value::Null);
        }
        _ => {}
    }
    Value::Object(object)
}

/// A word, as the file writes it.
fn word<T: serde::Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// A run of points, as the file writes it.
#[must_use]
pub fn points_json(points: &[Point]) -> Value {
    Value::Array(
        points
            .iter()
            .map(|point| Value::Array(vec![Number::json(point.x), Number::json(point.y)]))
            .collect(),
    )
}

/// A rounding, as the file writes it, for a change to an element that already exists.
#[must_use]
pub fn roundness_value(kind: Kind, roundness: Option<Roundness>) -> Value {
    match roundness {
        None => Value::Null,
        Some(held) if !kind.can_be_round() => {
            let _ = held;
            Value::Null
        }
        Some(held) => {
            let mut object = Map::new();
            let which = match held {
                Roundness::Legacy => 1,
                Roundness::Proportional => 2,
                Roundness::Adaptive { .. } => 3,
            };
            object.insert("type".to_owned(), Value::from(which));
            if let Roundness::Adaptive { value: Some(value) } = held {
                object.insert("value".to_owned(), Number::json(value));
            }
            Value::Object(object)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn made(kind: Kind) -> Value {
        element(
            kind,
            Box_ {
                x: 10.0,
                y: 20.0,
                width: 100.0,
                height: 50.0,
            },
            &Style::default(),
            &Id::new("abc"),
            Seed(1234),
            5678,
            1_756_304_871_234,
        )
    }

    #[test]
    fn a_new_element_reads_back_as_what_it_was_made_as() {
        let value = made(Kind::Rectangle);
        let held = crate::element::read(value.as_object().expect("an object")).expect("an element");
        assert_eq!(held.kind, Kind::Rectangle);
        assert_eq!(held.id.as_str(), "abc");
        assert_eq!(held.seed, Seed(1234));
        assert!((held.x - 10.0).abs() < f64::EPSILON);
        assert_eq!(held.version, 1);
    }

    #[test]
    fn the_keys_are_written_in_the_order_excalidraw_writes_them() {
        let value = made(Kind::Rectangle);
        let keys: Vec<&str> = value
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            &keys[..8],
            [
                "id",
                "type",
                "x",
                "y",
                "width",
                "height",
                "angle",
                "strokeColor"
            ]
        );
        assert_eq!(keys[keys.len() - 1], "locked");
    }

    #[test]
    fn a_rectangle_rounds_adaptively_and_a_diamond_proportionally() {
        let rectangle = made(Kind::Rectangle);
        assert_eq!(rectangle["roundness"]["type"], serde_json::json!(3));
        let diamond = made(Kind::Diamond);
        assert_eq!(diamond["roundness"]["type"], serde_json::json!(2));
        let ellipse = made(Kind::Ellipse);
        assert_eq!(
            ellipse["roundness"],
            Value::Null,
            "an ellipse has no corners"
        );
    }

    #[test]
    fn a_new_arrow_has_a_head_and_a_new_line_has_none() {
        let arrow = made(Kind::Arrow);
        assert_eq!(arrow["endArrowhead"], serde_json::json!("arrow"));
        assert_eq!(arrow["startArrowhead"], Value::Null);
        let line = made(Kind::Line);
        assert_eq!(line["endArrowhead"], Value::Null);
        assert_eq!(line["polygon"], serde_json::json!(false));
    }

    #[test]
    fn a_new_text_carries_everything_words_need() {
        let text = made(Kind::Text);
        let held = crate::element::read(text.as_object().expect("an object")).expect("an element");
        let words = held.text().expect("text");
        assert!((words.font_size - 20.0).abs() < f64::EPSILON);
        assert_eq!(words.font_family, FontFamily::Excalifont);
        assert!(words.auto_resize);
    }

    #[test]
    fn a_whole_coordinate_is_written_without_a_point() {
        let value = made(Kind::Rectangle);
        assert_eq!(value["x"].to_string(), "10");
        assert_eq!(value["width"].to_string(), "100");
    }
}
