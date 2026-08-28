//! What a drawn shape is made of.
//!
//! rough.js answers a list of moves and cubic curves rather than a path, because the arrowhead
//! code reads the curve back out to find which way the line points. The list is kept in that shape
//! for the same reason, and [`crate::to_path`] turns it into geometry when something wants to draw
//! it.

use kurbo::Point;

/// One step of a drawn outline.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Op {
    /// Lift the pen and put it down at this point.
    Move(Point),
    /// A cubic curve to the last point, through the two before it.
    Curve(Point, Point, Point),
    /// A straight line. Only a fill draws these.
    Line(Point),
}

impl Op {
    /// Where the pen ends up.
    #[must_use]
    pub const fn end(&self) -> Point {
        match self {
            Self::Move(at) | Self::Line(at) => *at,
            Self::Curve(_, _, at) => *at,
        }
    }
}

/// What one list of ops is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpSetKind {
    /// The outline, stroked.
    Path,
    /// The inside, filled.
    FillPath,
    /// The inside, drawn as strokes.
    FillSketch,
}

/// One list of ops, and what it is for.
#[derive(Clone, PartialEq, Debug)]
pub struct OpSet {
    /// What it is for.
    pub kind: OpSetKind,
    /// The steps, in order.
    pub ops: Vec<Op>,
}

impl OpSet {
    /// An empty set of `kind`.
    #[must_use]
    pub const fn new(kind: OpSetKind) -> Self {
        Self {
            kind,
            ops: Vec::new(),
        }
    }

    /// A set of `kind` over `ops`.
    #[must_use]
    pub const fn from_ops(kind: OpSetKind, ops: Vec<Op>) -> Self {
        Self { kind, ops }
    }

    /// Whether it draws nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

/// One shape, drawn.
///
/// The sets are in the order they are painted: the fill first, then the outline over it.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Drawable {
    /// What it is made of.
    pub sets: Vec<OpSet>,
}

impl Drawable {
    /// The first set that is an outline, which is what an arrowhead is aimed along.
    #[must_use]
    pub fn outline(&self) -> Option<&OpSet> {
        self.sets.iter().find(|set| set.kind == OpSetKind::Path)
    }
}
