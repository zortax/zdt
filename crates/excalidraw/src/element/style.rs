//! The words a drawing uses for how a thing looks.
//!
//! Each is exactly the string the file holds. An unknown one falls back to the default rather than
//! failing the read, so a drawing made by a newer Excalidraw still opens.

use serde::{Deserialize, Serialize};

/// How the inside of a shape is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FillStyle {
    /// Parallel strokes.
    Hachure,
    /// Two sets of parallel strokes, a quarter turn apart.
    CrossHatch,
    /// A filled outline.
    #[default]
    Solid,
    /// Parallel strokes that zig back and forth.
    ZigZag,
}

impl FillStyle {
    /// The same style, as the drawing library names it.
    #[must_use]
    pub const fn to_rough(self) -> excalidraw_rough::FillStyle {
        match self {
            Self::Hachure => excalidraw_rough::FillStyle::Hachure,
            Self::CrossHatch => excalidraw_rough::FillStyle::CrossHatch,
            Self::Solid => excalidraw_rough::FillStyle::Solid,
            Self::ZigZag => excalidraw_rough::FillStyle::ZigZag,
        }
    }
}

/// How the outline of a shape is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StrokeStyle {
    /// One unbroken line.
    #[default]
    Solid,
    /// Long marks.
    Dashed,
    /// Short ones.
    Dotted,
}

impl StrokeStyle {
    /// The dashes this style is drawn with at `width`, when it has any.
    #[must_use]
    pub fn dashes(self, width: f64) -> Option<[f64; 2]> {
        match self {
            Self::Solid => None,
            Self::Dashed => Some([8.0, 8.0 + width]),
            Self::Dotted => Some([1.5, 6.0 + width]),
        }
    }
}

/// Where the words sit across their box.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextAlign {
    /// Against the left edge.
    #[default]
    Left,
    /// In the middle.
    Center,
    /// Against the right edge.
    Right,
}

/// Where they sit down it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VerticalAlign {
    /// Against the top edge.
    #[default]
    Top,
    /// In the middle.
    Middle,
    /// Against the bottom edge.
    Bottom,
}

/// How the corners of a shape are cut.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Roundness {
    /// A quarter of the shorter side. What old files with round corners use.
    Legacy,
    /// The same, for linear elements and diamonds.
    Proportional,
    /// A fixed radius, until the shape is small enough that a quarter is smaller.
    Adaptive {
        /// The radius, when the file names one.
        value: Option<f64>,
    },
}

/// A quarter of the shorter side.
pub const PROPORTIONAL_RADIUS: f64 = 0.25;
/// The radius an adaptive corner is cut to, in pixels.
pub const ADAPTIVE_RADIUS: f64 = 32.0;

impl Roundness {
    /// The radius this rounding cuts from a side of length `side`.
    #[must_use]
    pub fn radius(self, side: f64) -> f64 {
        match self {
            Self::Legacy | Self::Proportional => side * PROPORTIONAL_RADIUS,
            Self::Adaptive { value } => {
                let fixed = value.unwrap_or(ADAPTIVE_RADIUS);
                // Below the cutoff a fixed radius would swallow the shape, so it goes back to a
                // proportion of it.
                let cutoff = fixed / PROPORTIONAL_RADIUS;
                if side <= cutoff {
                    side * PROPORTIONAL_RADIUS
                } else {
                    fixed
                }
            }
        }
    }
}

/// Which end of a line is decorated, and how.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arrowhead {
    /// Two barbs.
    Arrow,
    /// A crossbar.
    Bar,
    /// A filled circle.
    Circle,
    /// An open one.
    CircleOutline,
    /// A filled triangle.
    Triangle,
    /// An open one.
    TriangleOutline,
    /// A filled diamond.
    Diamond,
    /// An open one.
    DiamondOutline,
    /// One, in the notation a database diagram uses.
    CardinalityOne,
    /// Many.
    CardinalityMany,
    /// One or many.
    CardinalityOneOrMany,
    /// Exactly one.
    CardinalityExactlyOne,
    /// Zero or one.
    CardinalityZeroOrOne,
    /// Zero or many.
    CardinalityZeroOrMany,
}

impl Arrowhead {
    /// The head an older file's word means.
    ///
    /// The crow's foot names were renamed when the cardinality set arrived, and `dot` became the
    /// filled circle.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        Some(match word {
            "arrow" => Self::Arrow,
            "bar" => Self::Bar,
            "dot" | "circle" => Self::Circle,
            "circle_outline" => Self::CircleOutline,
            "triangle" => Self::Triangle,
            "triangle_outline" => Self::TriangleOutline,
            "diamond" => Self::Diamond,
            "diamond_outline" => Self::DiamondOutline,
            "crowfoot_one" | "cardinality_one" => Self::CardinalityOne,
            "crowfoot_many" | "cardinality_many" => Self::CardinalityMany,
            "crowfoot_one_or_many" | "cardinality_one_or_many" => Self::CardinalityOneOrMany,
            "cardinality_exactly_one" => Self::CardinalityExactlyOne,
            "cardinality_zero_or_one" => Self::CardinalityZeroOrOne,
            "cardinality_zero_or_many" => Self::CardinalityZeroOrMany,
            _ => return None,
        })
    }

    /// The word the file holds for this head.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Arrow => "arrow",
            Self::Bar => "bar",
            Self::Circle => "circle",
            Self::CircleOutline => "circle_outline",
            Self::Triangle => "triangle",
            Self::TriangleOutline => "triangle_outline",
            Self::Diamond => "diamond",
            Self::DiamondOutline => "diamond_outline",
            Self::CardinalityOne => "cardinality_one",
            Self::CardinalityMany => "cardinality_many",
            Self::CardinalityOneOrMany => "cardinality_one_or_many",
            Self::CardinalityExactlyOne => "cardinality_exactly_one",
            Self::CardinalityZeroOrOne => "cardinality_zero_or_one",
            Self::CardinalityZeroOrMany => "cardinality_zero_or_many",
        }
    }

    /// How large it is drawn.
    #[must_use]
    pub const fn size(self) -> f64 {
        match self {
            Self::Arrow => 25.0,
            Self::Diamond | Self::DiamondOutline => 12.0,
            Self::CardinalityOne | Self::CardinalityExactlyOne | Self::CardinalityZeroOrOne => 20.0,
            _ => 15.0,
        }
    }

    /// How far its barbs are turned from the line, in degrees.
    #[must_use]
    pub const fn angle(self) -> f64 {
        match self {
            Self::Bar => 90.0,
            Self::Arrow => 20.0,
            _ => 25.0,
        }
    }
}

/// How rough a shape is drawn.
pub mod roughness {
    /// Ruled.
    pub const ARCHITECT: f64 = 0.0;
    /// Freehand, which is the default.
    pub const ARTIST: f64 = 1.0;
    /// Loose.
    pub const CARTOONIST: f64 = 2.0;
}

/// How wide an outline is drawn.
pub mod stroke_width {
    /// Thin.
    pub const THIN: f64 = 1.0;
    /// The default.
    pub const MEDIUM: f64 = 2.0;
    /// Bold.
    pub const BOLD: f64 = 4.0;

    /// The same three, for a freehand stroke, which is drawn at half the width.
    pub const FREEDRAW: [f64; 3] = [0.5, 1.0, 2.0];
    /// And for everything else.
    pub const SHAPE: [f64; 3] = [THIN, MEDIUM, BOLD];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_adaptive_corner_is_fixed_until_the_shape_is_small() {
        let adaptive = Roundness::Adaptive { value: None };
        // Above the cutoff of 32 / 0.25.
        assert!((adaptive.radius(220.0) - 32.0).abs() < f64::EPSILON);
        // At it and below, a quarter of the side.
        assert!((adaptive.radius(128.0) - 32.0).abs() < f64::EPSILON);
        assert!((adaptive.radius(40.0) - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_proportional_corner_is_always_a_quarter() {
        assert!((Roundness::Proportional.radius(220.0) - 55.0).abs() < f64::EPSILON);
        assert!((Roundness::Legacy.radius(220.0) - 55.0).abs() < f64::EPSILON);
    }

    #[test]
    fn the_old_arrowhead_names_still_read() {
        assert_eq!(Arrowhead::parse("dot"), Some(Arrowhead::Circle));
        assert_eq!(
            Arrowhead::parse("crowfoot_many"),
            Some(Arrowhead::CardinalityMany)
        );
        assert_eq!(Arrowhead::parse("nonsense"), None);
    }

    #[test]
    fn every_arrowhead_writes_the_word_it_was_read_from() {
        for head in [
            Arrowhead::Arrow,
            Arrowhead::Bar,
            Arrowhead::Circle,
            Arrowhead::CircleOutline,
            Arrowhead::Triangle,
            Arrowhead::TriangleOutline,
            Arrowhead::Diamond,
            Arrowhead::DiamondOutline,
            Arrowhead::CardinalityOne,
            Arrowhead::CardinalityMany,
            Arrowhead::CardinalityOneOrMany,
            Arrowhead::CardinalityExactlyOne,
            Arrowhead::CardinalityZeroOrOne,
            Arrowhead::CardinalityZeroOrMany,
        ] {
            assert_eq!(Arrowhead::parse(head.as_str()), Some(head));
        }
    }

    #[test]
    fn dashes_widen_with_the_stroke() {
        assert_eq!(StrokeStyle::Solid.dashes(2.0), None);
        assert_eq!(StrokeStyle::Dashed.dashes(2.0), Some([8.0, 10.0]));
        assert_eq!(StrokeStyle::Dotted.dashes(2.0), Some([1.5, 8.0]));
    }
}
