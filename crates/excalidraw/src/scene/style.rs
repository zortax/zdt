//! One change to how a set of elements looks.

use crate::element::FontFamily;
use crate::element::style::{
    Arrowhead, FillStyle, Roundness, StrokeStyle, TextAlign, VerticalAlign,
};

/// What a restyling changes.
///
/// One field per control on the properties panel, so a press is one variant and one command.
#[derive(Clone, PartialEq, Debug)]
pub enum Change {
    /// What the outline is drawn in.
    StrokeColor(String),
    /// What the inside is filled with.
    BackgroundColor(String),
    /// How that inside is drawn.
    FillStyle(FillStyle),
    /// How wide the outline is.
    StrokeWidth(f64),
    /// How it is broken up.
    StrokeStyle(StrokeStyle),
    /// How far the hand wanders.
    Roughness(f64),
    /// How solid it is, from nothing to a hundred.
    Opacity(f64),
    /// How the corners are cut.
    Roundness(Option<Roundness>),
    /// How tall the letters are.
    FontSize(f64),
    /// Which face they are in.
    FontFamily(FontFamily),
    /// Where they sit across their box.
    TextAlign(TextAlign),
    /// And down it.
    VerticalAlign(VerticalAlign),
    /// What decorates the start of a line.
    StartArrowhead(Option<Arrowhead>),
    /// And its end.
    EndArrowhead(Option<Arrowhead>),
    /// Whether the reader may move it.
    Locked(bool),
    /// Where it points.
    Link(Option<String>),
}

impl Change {
    /// Whether this change means anything to an element of `kind`.
    ///
    /// A change that means nothing writes nothing, so setting the font size with a rectangle
    /// selected leaves the rectangle exactly as it was.
    #[must_use]
    pub const fn applies_to(&self, kind: crate::element::Kind) -> bool {
        use crate::element::Kind as K;
        match self {
            Self::FontSize(_) | Self::FontFamily(_) | Self::TextAlign(_) => {
                matches!(kind, K::Text)
            }
            // Where words sit down their box is the container's business as well as the words'.
            Self::VerticalAlign(_) => matches!(kind, K::Text) || kind.is_text_container(),
            Self::StartArrowhead(_) | Self::EndArrowhead(_) => matches!(kind, K::Arrow),
            Self::Roundness(_) => kind.can_be_round(),
            Self::FillStyle(_) | Self::BackgroundColor(_) => !matches!(kind, K::Text),
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::Kind;

    #[test]
    fn a_font_change_only_reaches_words() {
        let change = Change::FontSize(28.0);
        assert!(change.applies_to(Kind::Text));
        assert!(!change.applies_to(Kind::Rectangle));
    }

    #[test]
    fn a_head_only_reaches_an_arrow() {
        let change = Change::EndArrowhead(Some(Arrowhead::Triangle));
        assert!(change.applies_to(Kind::Arrow));
        assert!(!change.applies_to(Kind::Line));
    }

    #[test]
    fn a_rounding_only_reaches_a_kind_that_has_corners() {
        let change = Change::Roundness(None);
        assert!(change.applies_to(Kind::Rectangle));
        assert!(!change.applies_to(Kind::Ellipse));
    }

    #[test]
    fn a_colour_reaches_everything() {
        let change = Change::StrokeColor("#e03131".to_owned());
        for kind in [Kind::Rectangle, Kind::Text, Kind::Arrow, Kind::Freedraw] {
            assert!(change.applies_to(kind));
        }
    }
}
