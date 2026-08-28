//! What a drawing is made of.
//!
//! An element is read from the JSON object the file holds, never written back to one: writing goes
//! through [`crate::store`], which keeps the object it was read from and patches the keys a change
//! touched. So this is a view — everything a reader, a renderer and a hit test need, in the types
//! they want it in — and unknown keys are the store's business rather than this one's.

pub mod font;
pub mod id;
pub mod read;
pub mod style;

use kurbo::Point;
use serde::Deserialize;
use serde_json::{Map, Value};

pub use self::font::FontFamily;
pub use self::id::{Id, Seed};
pub use self::read::element as read;
pub use self::style::{Arrowhead, FillStyle, Roundness, StrokeStyle, TextAlign, VerticalAlign};

/// Which kind of thing an element is.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A rectangle.
    Rectangle,
    /// A diamond.
    Diamond,
    /// An ellipse.
    Ellipse,
    /// A run of straight or curved segments.
    Line,
    /// The same, with a head at one or both ends.
    Arrow,
    /// A pen stroke.
    Freedraw,
    /// Words.
    Text,
    /// A picture.
    Image,
    /// A named box other elements belong to.
    Frame,
    /// The same, for a generated drawing.
    Magicframe,
    /// A web page, shown in place.
    Embeddable,
    /// The same, in a frame of its own.
    Iframe,
    /// The rubber band of a selection. Legacy; a file that holds one has it dropped.
    Selection,
}

impl Kind {
    /// Whether an arrow may bind to this kind.
    #[must_use]
    pub const fn is_bindable(self) -> bool {
        matches!(
            self,
            Self::Rectangle
                | Self::Diamond
                | Self::Ellipse
                | Self::Text
                | Self::Image
                | Self::Iframe
                | Self::Embeddable
                | Self::Frame
                | Self::Magicframe
        )
    }

    /// Whether words may be bound inside this kind.
    #[must_use]
    pub const fn is_text_container(self) -> bool {
        matches!(
            self,
            Self::Rectangle | Self::Diamond | Self::Ellipse | Self::Arrow
        )
    }

    /// Whether this kind is drawn from a run of points rather than from a box.
    #[must_use]
    pub const fn is_linear(self) -> bool {
        matches!(self, Self::Line | Self::Arrow)
    }

    /// Which rounding this kind uses when a file only says that it is round.
    #[must_use]
    pub const fn adaptive_radius(self) -> bool {
        matches!(
            self,
            Self::Rectangle | Self::Embeddable | Self::Iframe | Self::Image
        )
    }

    /// Whether the corners of this kind can be rounded at all.
    #[must_use]
    pub const fn can_be_round(self) -> bool {
        matches!(
            self,
            Self::Rectangle
                | Self::Diamond
                | Self::Line
                | Self::Arrow
                | Self::Embeddable
                | Self::Iframe
                | Self::Image
        )
    }

    /// The word the file holds for this kind.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rectangle => "rectangle",
            Self::Diamond => "diamond",
            Self::Ellipse => "ellipse",
            Self::Line => "line",
            Self::Arrow => "arrow",
            Self::Freedraw => "freedraw",
            Self::Text => "text",
            Self::Image => "image",
            Self::Frame => "frame",
            Self::Magicframe => "magicframe",
            Self::Embeddable => "embeddable",
            Self::Iframe => "iframe",
            Self::Selection => "selection",
        }
    }
}

/// What another element is to this one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoundKind {
    /// An arrow whose end is fixed to it.
    Arrow,
    /// Words written inside it.
    Text,
}

/// One element bound to another.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Bound {
    /// Which element.
    pub id: Id,
    /// What it is to this one.
    pub kind: BoundKind,
}

/// How close to a shape a bound arrow is allowed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BindMode {
    /// All the way to the fixed point, inside the shape.
    Inside,
    /// Outside the outline, held off by the binding gap.
    #[default]
    Orbit,
    /// Not snapped at all.
    Skip,
}

/// Where an arrow's end is fixed to a shape.
#[derive(Clone, PartialEq, Debug)]
pub struct Binding {
    /// Which shape.
    pub element: Id,
    /// Where on it, as a fraction of its width and height.
    pub fixed_point: (f64, f64),
    /// How close the end is allowed.
    pub mode: BindMode,
}

/// One corner of the box an image was cut from.
#[derive(Clone, Copy, PartialEq, Debug, Deserialize)]
pub struct Crop {
    /// How far in from the left, in the picture's own pixels.
    pub x: f64,
    /// How far down.
    pub y: f64,
    /// How wide the cut is.
    pub width: f64,
    /// How tall.
    pub height: f64,
    /// How wide the whole picture is.
    #[serde(rename = "naturalWidth")]
    pub natural_width: f64,
    /// How tall.
    #[serde(rename = "naturalHeight")]
    pub natural_height: f64,
}

/// Whether a picture's bytes have been filed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileStatus {
    /// Not yet.
    #[default]
    Pending,
    /// Yes.
    Saved,
    /// They could not be.
    Error,
}

/// One straight run of an elbowed arrow the reader may not move.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FixedSegment {
    /// Where it starts, in the element's own coordinates.
    pub start: Point,
    /// Where it ends.
    pub end: Point,
    /// Which of the arrow's points it begins at.
    pub index: usize,
}

/// What is true of every element.
#[derive(Clone, PartialEq, Debug)]
pub struct Element {
    /// Which element this is.
    pub id: Id,
    /// What kind of thing it is.
    pub kind: Kind,
    /// Where its unrotated box begins.
    pub x: f64,
    /// The same, down the page.
    pub y: f64,
    /// How wide that box is.
    pub width: f64,
    /// How tall.
    pub height: f64,
    /// How far it is turned about the middle of its box, in radians.
    pub angle: f64,
    /// What its outline is drawn in.
    pub stroke_color: String,
    /// What its inside is filled with. `transparent` is no fill.
    pub background_color: String,
    /// How that inside is drawn.
    pub fill_style: FillStyle,
    /// How wide the outline is.
    pub stroke_width: f64,
    /// How that outline is broken up.
    pub stroke_style: StrokeStyle,
    /// How far the hand drawing it wanders.
    pub roughness: f64,
    /// How solid it is, from nothing to a hundred.
    pub opacity: f64,
    /// Which groups it is in, innermost first.
    pub group_ids: Vec<String>,
    /// Which frame it belongs to.
    pub frame_id: Option<Id>,
    /// How its corners are cut.
    pub roundness: Option<Roundness>,
    /// What its wobble is drawn from.
    pub seed: Seed,
    /// How many times it has been changed.
    pub version: u64,
    /// What settles a tie between two changes at the same version.
    pub version_nonce: u64,
    /// Where it sits in the order, as a key that sorts.
    pub index: Option<String>,
    /// Whether it has been removed. A removed element stays in the file.
    pub is_deleted: bool,
    /// What is fixed to it.
    pub bound_elements: Vec<Bound>,
    /// When it was last changed, as milliseconds since the epoch.
    pub updated: u64,
    /// Where it points, when it does.
    pub link: Option<String>,
    /// Whether the reader may move it.
    pub locked: bool,
    /// What kind it is, and what only that kind has.
    pub data: Data,
}

/// What only one kind of element has.
#[derive(Clone, PartialEq, Debug, Default)]
pub enum Data {
    /// A rectangle, a diamond or an ellipse, which have nothing of their own.
    #[default]
    Shape,
    /// A line or an arrow.
    Linear(Linear),
    /// A pen stroke.
    Freedraw(Freedraw),
    /// Words.
    Text(Text),
    /// A picture.
    Image(Image),
    /// A named box.
    Frame(Frame),
    /// A web page.
    Embed,
}

/// What a line or an arrow has.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Linear {
    /// Where it goes, relative to `x` and `y`. The first is always the origin.
    pub points: Vec<Point>,
    /// Which shape its first end is fixed to.
    pub start_binding: Option<Binding>,
    /// And its last.
    pub end_binding: Option<Binding>,
    /// What decorates its first end.
    pub start_arrowhead: Option<Arrowhead>,
    /// And its last.
    pub end_arrowhead: Option<Arrowhead>,
    /// Whether it is routed in right angles.
    pub elbowed: bool,
    /// The runs of an elbowed arrow the reader has fixed.
    pub fixed_segments: Vec<FixedSegment>,
    /// Whether the first run of an elbowed arrow is hidden.
    pub start_is_special: bool,
    /// Whether its last is.
    pub end_is_special: bool,
    /// Whether a line is closed and filled.
    pub polygon: bool,
}

/// What a pen stroke has.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Freedraw {
    /// Where the pen went, relative to `x` and `y`.
    pub points: Vec<Point>,
    /// How hard it was pressed at each point.
    pub pressures: Vec<f64>,
    /// Whether to read pressure from how fast it moved instead.
    pub simulate_pressure: bool,
    /// How much the input is pulled towards a smooth line.
    pub streamline: f64,
    /// How its width varies along it.
    pub variability: excalidraw_rough::Variability,
}

/// What written words have.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Text {
    /// What is drawn, with the line breaks the wrapping put in.
    pub text: String,
    /// What was typed, without them.
    pub original_text: String,
    /// How tall the letters are.
    pub font_size: f64,
    /// Which face they are drawn in.
    pub font_family: FontFamily,
    /// Where they sit across their box.
    pub text_align: TextAlign,
    /// And down it.
    pub vertical_align: VerticalAlign,
    /// Which shape they are written inside, when they are inside one.
    pub container_id: Option<Id>,
    /// Whether the box grows with the words rather than the words wrapping in it.
    pub auto_resize: bool,
    /// How far apart the lines are, as a multiple of the font size.
    pub line_height: f64,
}

/// What a picture has.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Image {
    /// Which of the file's pictures it draws.
    pub file_id: Option<String>,
    /// Whether those bytes have been filed.
    pub status: FileStatus,
    /// How it is flipped, as a sign on each axis.
    pub scale: (f64, f64),
    /// Which part of the picture is shown.
    pub crop: Option<Crop>,
}

/// What a named box has.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Frame {
    /// What it is called.
    pub name: Option<String>,
}

impl Element {
    /// The line or arrow this is, when it is one.
    #[must_use]
    pub const fn linear(&self) -> Option<&Linear> {
        match &self.data {
            Data::Linear(held) => Some(held),
            _ => None,
        }
    }

    /// The pen stroke this is, when it is one.
    #[must_use]
    pub const fn freedraw(&self) -> Option<&Freedraw> {
        match &self.data {
            Data::Freedraw(held) => Some(held),
            _ => None,
        }
    }

    /// The words this is, when it is words.
    #[must_use]
    pub const fn text(&self) -> Option<&Text> {
        match &self.data {
            Data::Text(held) => Some(held),
            _ => None,
        }
    }

    /// The picture this is, when it is one.
    #[must_use]
    pub const fn image(&self) -> Option<&Image> {
        match &self.data {
            Data::Image(held) => Some(held),
            _ => None,
        }
    }

    /// The frame this is, when it is one.
    #[must_use]
    pub const fn frame(&self) -> Option<&Frame> {
        match &self.data {
            Data::Frame(held) => Some(held),
            _ => None,
        }
    }

    /// Whether it has an inside to fill.
    #[must_use]
    pub fn is_filled(&self) -> bool {
        !is_transparent(&self.background_color)
    }

    /// How solid it is drawn, from nothing to one.
    #[must_use]
    pub fn alpha(&self) -> f64 {
        (self.opacity / 100.0).clamp(0.0, 1.0)
    }

    /// The outermost group it is in, which is what a click selects.
    #[must_use]
    pub fn outermost_group(&self) -> Option<&str> {
        self.group_ids.last().map(String::as_str)
    }

    /// What its wobble is drawn from, as the drawing library wants it.
    #[must_use]
    pub const fn rough_seed(&self) -> u32 {
        self.seed.0
    }

    /// The words written inside it, when any are.
    #[must_use]
    pub fn bound_text(&self) -> Option<&Id> {
        self.bound_elements
            .iter()
            .find(|bound| bound.kind == BoundKind::Text)
            .map(|bound| &bound.id)
    }
}

/// Whether a colour draws nothing.
#[must_use]
pub fn is_transparent(color: &str) -> bool {
    color.eq_ignore_ascii_case("transparent") || color.is_empty()
}

/// A number out of a JSON object, when it holds one there.
pub(crate) fn number(object: &Map<String, Value>, key: &str) -> Option<f64> {
    object.get(key).and_then(Value::as_f64)
}

/// A string out of one.
pub(crate) fn string(object: &Map<String, Value>, key: &str) -> Option<String> {
    object.get(key).and_then(Value::as_str).map(str::to_owned)
}

/// A flag out of one.
pub(crate) fn flag(object: &Map<String, Value>, key: &str) -> Option<bool> {
    object.get(key).and_then(Value::as_bool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparent_is_the_only_colour_that_draws_nothing() {
        assert!(is_transparent("transparent"));
        assert!(is_transparent("TRANSPARENT"));
        assert!(is_transparent(""));
        assert!(!is_transparent("#ffffff"));
    }

    #[test]
    fn a_rectangle_rounds_adaptively_and_a_diamond_proportionally() {
        assert!(Kind::Rectangle.adaptive_radius());
        assert!(!Kind::Diamond.adaptive_radius());
        assert!(Kind::Diamond.can_be_round());
        assert!(!Kind::Ellipse.can_be_round());
    }

    #[test]
    fn only_some_kinds_take_an_arrow_or_a_label() {
        assert!(Kind::Rectangle.is_bindable());
        assert!(Kind::Rectangle.is_text_container());
        assert!(Kind::Arrow.is_text_container());
        assert!(!Kind::Arrow.is_bindable());
        assert!(!Kind::Freedraw.is_bindable());
    }
}
