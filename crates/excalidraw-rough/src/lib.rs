//! The hand-drawn geometry an Excalidraw drawing is made of.
//!
//! Two things are drawn by hand. Shapes are strokes that wander away from where they were asked
//! for, seeded so that the same file draws the same way every time; that is rough.js, ported here.
//! Freehand strokes are the outline of a pen whose width follows how hard it was pressed; that is
//! perfect-freehand, which the `freedraw` crate already provides and this crate only configures.
//!
//! Nothing here knows what an Excalidraw element is. A caller maps an element onto [`Options`] and
//! asks for the shape it names.
//!
//! ```
//! use excalidraw_rough::{Options, shape, to_path};
//!
//! let options = Options { seed: 1_263_748_391, roughness: 1.0, ..Options::default() };
//! let mut random = options.random();
//! let drawn = shape::rectangle(0.0, 0.0, 220.0, 128.0, &options, &mut random);
//! let painted = to_path::of_drawable(&drawn);
//! assert_eq!(painted.len(), 1, "an unfilled rectangle is one outline");
//! ```

pub mod fill;
pub mod freehand;
pub mod ops;
pub mod options;
pub mod random;
pub mod renderer;
pub mod shape;
pub mod to_path;

pub use kurbo;

pub use crate::freehand::{Stroke, Variability};
pub use crate::ops::{Drawable, Op, OpSet, OpSetKind};
pub use crate::options::{FillStyle, Options};
pub use crate::random::Random;
