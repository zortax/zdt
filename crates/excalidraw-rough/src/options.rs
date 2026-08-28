//! What one shape is drawn with.
//!
//! The fields are rough.js's, and the defaults are the ones an Excalidraw element never sets and
//! therefore always gets. A caller fills in what the element says and leaves the rest.

use crate::random::Random;

/// How a shape's inside is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FillStyle {
    /// Parallel strokes.
    #[default]
    Hachure,
    /// Two sets of parallel strokes, a quarter turn apart.
    CrossHatch,
    /// A filled outline.
    Solid,
    /// Parallel strokes that zig back and forth.
    ZigZag,
    /// A row of dots.
    Dots,
    /// Parallel strokes, dashed.
    Dashed,
    /// Parallel zigzag lines.
    ZigZagLine,
}

/// What one shape is drawn with.
///
/// Every field is what rough.js calls it. The defaults are rough.js's own, which is what an
/// Excalidraw element that says nothing about a field is drawn with.
#[derive(Clone, Debug)]
pub struct Options {
    /// The seed the wobble comes from.
    pub seed: u32,
    /// How far a stroke may wander, before roughness.
    pub max_randomness_offset: f64,
    /// How much of that wander is used. Zero draws the exact shape.
    pub roughness: f64,
    /// How far a straight line bends away from its ends.
    pub bowing: f64,
    /// How wide the outline is drawn.
    pub stroke_width: f64,
    /// How the inside is drawn.
    pub fill_style: FillStyle,
    /// Whether there is an inside to draw at all.
    pub filled: bool,
    /// How wide a fill stroke is. Negative asks for half the stroke width.
    pub fill_weight: f64,
    /// Which way the fill strokes run, in degrees.
    pub hachure_angle: f64,
    /// How far apart they are. Negative asks for four times the stroke width.
    pub hachure_gap: f64,
    /// How closely a curve follows the points it is fitted to. One is exactly.
    pub curve_fitting: f64,
    /// How tightly a curve is pulled towards its points. Zero is the loosest.
    pub curve_tightness: f64,
    /// How many segments the smallest curve is drawn with.
    pub curve_step_count: f64,
    /// Whether to draw one stroke where two would be drawn.
    pub disable_multi_stroke: bool,
    /// The same, for the strokes a fill is made of.
    pub disable_multi_stroke_fill: bool,
    /// Whether the ends of each stroke stay where they were asked for.
    pub preserve_vertices: bool,
    /// How much rougher a solid fill is than the outline around it.
    pub fill_shape_roughness_gain: f64,
    /// How far a dashed fill's dashes are apart. Negative asks for the hachure gap.
    pub dash_offset: f64,
    /// How long the gaps in one are. Negative asks for the hachure gap.
    pub dash_gap: f64,
    /// How far a zigzag fill's line swings. Negative asks for the hachure gap.
    pub zigzag_offset: f64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            seed: 0,
            max_randomness_offset: 2.0,
            roughness: 1.0,
            bowing: 1.0,
            stroke_width: 1.0,
            fill_style: FillStyle::Hachure,
            filled: false,
            fill_weight: -1.0,
            hachure_angle: -41.0,
            hachure_gap: -1.0,
            curve_fitting: 0.95,
            curve_tightness: 0.0,
            curve_step_count: 9.0,
            disable_multi_stroke: false,
            disable_multi_stroke_fill: false,
            preserve_vertices: false,
            fill_shape_roughness_gain: 0.8,
            dash_offset: -1.0,
            dash_gap: -1.0,
            zigzag_offset: -1.0,
        }
    }
}

impl Options {
    /// The generator this shape's wobble comes from.
    #[must_use]
    pub const fn random(&self) -> Random {
        Random::new(self.seed)
    }

    /// The same options, drawn from the next seed.
    ///
    /// rough.js draws the second stroke of a curve from this, so the two strokes wander apart
    /// rather than lying on top of each other.
    #[must_use]
    pub fn next_seed(&self) -> Self {
        Self {
            seed: self.seed.wrapping_add(1),
            ..self.clone()
        }
    }

    /// How wide one fill stroke is.
    #[must_use]
    pub fn fill_weight(&self) -> f64 {
        if self.fill_weight < 0.0 {
            self.stroke_width / 2.0
        } else {
            self.fill_weight
        }
    }

    /// How far apart the fill strokes are, never closer than a tenth of a pixel.
    #[must_use]
    pub fn hachure_gap(&self) -> f64 {
        let gap = if self.hachure_gap < 0.0 {
            self.stroke_width * 4.0
        } else {
            self.hachure_gap
        };
        gap.max(0.1).round()
    }
}
