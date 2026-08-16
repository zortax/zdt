//! The file tree, drawn.
//!
//! A virtualised list over the flattened tree, so a directory somebody expanded by accident costs
//! a few dozen rows. A hundred thousand never get built.
//!
//! The panel stays mounted and a style hides it when it is closed, the way an inactive buffer is
//! hidden. Toggling it is then a restyle, and the caret is where it was left.
//!
//! # Where its keys come from
//!
//! The same keymap as everything else, with the tree's own rows in front of it. That is what lets
//! `d` delete a file here and stay the delete operator everywhere else. The tree says nothing
//! about `<Leader>ff`, so it still works with the keyboard in the panel.

mod resize;
mod rows;

pub use crate::explorer::tree::resize::{TreeResize, TreeResizeProps};
pub use crate::explorer::tree::rows::{Explorer, ExplorerProps};

/// How tall one row is. The list is told, and measures nothing.
const ROW: f32 = 22.0;
