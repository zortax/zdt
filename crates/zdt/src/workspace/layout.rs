//! How the windows are arranged.
//!
//! A tree of splits with a window at each leaf, which is what `:split` and `:vsplit` build and
//! what `<C-w>` walks. Sizes are percentages of the split they are in, so resizing the window
//! keeps the proportions the user set — and so that the tree can be handed to a resizable panel
//! group unchanged.

use slotmap::new_key_type;

new_key_type! {
    /// Which window this is. One window shows one buffer.
    pub struct WindowId;
}

/// Which way a split divides its children.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    /// Side by side, the way `:vsplit` divides.
    Horizontal,
    /// One above the other, the way `:split` divides.
    Vertical,
}

impl Axis {
    /// The other one.
    #[must_use]
    pub const fn other(self) -> Self {
        match self {
            Self::Horizontal => Self::Vertical,
            Self::Vertical => Self::Horizontal,
        }
    }
}

/// Which way `<C-w>h` and its neighbours look.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    /// `<C-w>h`.
    Left,
    /// `<C-w>l`.
    Right,
    /// `<C-w>k`.
    Up,
    /// `<C-w>j`.
    Down,
}

impl Direction {
    /// Which the direction a split has to divide along for this to cross it.
    #[must_use]
    pub const fn axis(self) -> Axis {
        match self {
            Self::Left | Self::Right => Axis::Horizontal,
            Self::Up | Self::Down => Axis::Vertical,
        }
    }

    /// Whether it looks toward the later children of a split rather than the earlier ones.
    #[must_use]
    pub const fn forward(self) -> bool {
        matches!(self, Self::Right | Self::Down)
    }

    /// The direction a keymap's argument names, when it names one.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "left" => Self::Left,
            "right" => Self::Right,
            "up" => Self::Up,
            "down" => Self::Down,
            _ => return None,
        })
    }
}

/// The arrangement of windows.
#[derive(Clone, PartialEq, Debug)]
pub enum Layout {
    /// One window.
    Leaf(WindowId),
    /// Several, divided along one axis, each with its share of the space.
    Split {
        /// Which way it divides.
        axis: Axis,
        /// What is in it, each with its percentage of the split.
        children: Vec<(Layout, f64)>,
    },
}

/// A [`Layout`] with the shares taken out. See [`Layout::shape`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Shape {
    /// One window.
    Leaf(WindowId),
    /// Several, divided along one axis.
    Split {
        /// Which way it divides.
        axis: Axis,
        /// What is in it.
        children: Vec<Shape>,
    },
}

impl Layout {
    /// Every window in the tree, left to right and top to bottom.
    ///
    /// The order `<C-w>w` walks and the order a session writes them down in.
    pub fn windows(&self) -> Vec<WindowId> {
        let mut found = Vec::new();
        self.collect(&mut found);
        found
    }

    /// The window `direction` of `from`, when there is one.
    ///
    /// Worked out from the tree rather than from where things ended up on screen: the nearest
    /// ancestor split that divides the right way is crossed, and the sibling on the other side is
    /// entered at its nearest edge. That is what vim does with a tree of splits, and it needs no
    /// geometry — which matters, because the geometry is not known until after a frame is drawn.
    #[must_use]
    pub fn neighbour(&self, from: WindowId, direction: Direction) -> Option<WindowId> {
        let mut path = Vec::new();
        if !self.path_to(from, &mut path) {
            return None;
        }

        // Climbing from the window: the first split that divides the way this direction crosses,
        // and has a sibling that way, is the one to step through.
        for depth in (0..path.len()).rev() {
            let (node, index) = &path[depth];
            let Self::Split { axis, children } = node else {
                continue;
            };
            if *axis != direction.axis() {
                continue;
            }
            let next = if direction.forward() {
                index + 1
            } else {
                index.checked_sub(1)?
            };
            if let Some((child, _)) = children.get(next) {
                return Some(child.edge(direction));
            }
        }
        None
    }

    /// The window at the near edge of this subtree, coming from `direction`.
    fn edge(&self, direction: Direction) -> WindowId {
        match self {
            Self::Leaf(id) => *id,
            Self::Split { axis, children } => {
                // Along the direction being travelled, enter at the near side; across it, the
                // first child is as good as any without knowing where the caret was.
                let at = if *axis == direction.axis() && direction.forward() {
                    children.first()
                } else if *axis == direction.axis() {
                    children.last()
                } else {
                    children.first()
                };
                at.map_or_else(
                    || match children.first() {
                        Some((child, _)) => child.edge(direction),
                        None => WindowId::default(),
                    },
                    |(child, _)| child.edge(direction),
                )
            }
        }
    }

    /// The nodes from the root down to the one holding `from`, each with the child index taken.
    fn path_to<'a>(&'a self, from: WindowId, into: &mut Vec<(&'a Self, usize)>) -> bool {
        match self {
            Self::Leaf(id) => *id == from,
            Self::Split { children, .. } => {
                for (index, (child, _)) in children.iter().enumerate() {
                    into.push((self, index));
                    if child.path_to(from, into) {
                        return true;
                    }
                    into.pop();
                }
                false
            }
        }
    }

    fn collect(&self, into: &mut Vec<WindowId>) {
        match self {
            Self::Leaf(id) => into.push(*id),
            Self::Split { children, .. } => {
                for (child, _) in children {
                    child.collect(into);
                }
            }
        }
    }

    /// How many windows there are.
    ///
    /// There is never nothing, so there is no `is_empty` beside this: an empty layout would be a
    /// window with nowhere to put the caret.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        match self {
            Self::Leaf(_) => 1,
            Self::Split { children, .. } => children.iter().map(|(child, _)| child.len()).sum(),
        }
    }

    /// Whether there is only one window.
    pub fn is_single(&self) -> bool {
        matches!(self, Self::Leaf(_))
    }

    /// Divides the split holding `at` along `axis`, putting `new` beside it.
    ///
    /// A split along the axis its parent already divides on joins that parent rather than nesting
    /// inside it — three vertical splits are one row of three, not a row of two with a row of two
    /// in it, which is what makes their sizes add up to what the user sees.
    pub fn split(&mut self, at: WindowId, axis: Axis, new: WindowId) -> bool {
        match self {
            Self::Leaf(id) if *id == at => {
                *self = Self::Split {
                    axis,
                    children: vec![(Self::Leaf(at), 50.0), (Self::Leaf(new), 50.0)],
                };
                true
            }
            Self::Leaf(_) => false,
            Self::Split {
                axis: held,
                children,
            } => {
                if *held == axis
                    && let Some(index) = children
                        .iter()
                        .position(|(child, _)| matches!(child, Self::Leaf(id) if *id == at))
                {
                    // The new window takes half of what the one it split had.
                    let share = children[index].1 / 2.0;
                    children[index].1 = share;
                    children.insert(index + 1, (Self::Leaf(new), share));
                    return true;
                }
                children
                    .iter_mut()
                    .any(|(child, _)| child.split(at, axis, new))
            }
        }
    }

    /// Removes `at`, giving its space to whatever was beside it.
    ///
    /// A split left with one child stops being a split: closing one of two windows leaves the
    /// other filling what both had, rather than a split of one that every later operation would
    /// have to know about.
    pub fn close(&mut self, at: WindowId) -> bool {
        let Self::Split { children, .. } = self else {
            // The last window is not closeable; something has to be on screen.
            return false;
        };

        if let Some(index) = children
            .iter()
            .position(|(child, _)| matches!(child, Self::Leaf(id) if *id == at))
        {
            let freed = children.remove(index).1;
            if let Some(neighbour) = children.get_mut(index.saturating_sub(1)) {
                neighbour.1 += freed;
            }
            if children.len() == 1 {
                *self = children.remove(0).0;
            }
            return true;
        }

        let closed = children.iter_mut().any(|(child, _)| child.close(at));
        if closed && children.len() == 1 {
            *self = children.remove(0).0;
        }
        closed
    }

    /// Writes new percentages into the split holding `at`.
    ///
    /// What a dragged handle reports: the panel group answers with every size in the group, and
    /// the group is one split of this tree.
    pub fn resize(&mut self, at: WindowId, sizes: &[f64]) -> bool {
        let Self::Split { children, .. } = self else {
            return false;
        };
        let holds = children
            .iter()
            .any(|(child, _)| matches!(child, Self::Leaf(id) if *id == at));
        if holds && children.len() == sizes.len() {
            for (child, size) in children.iter_mut().zip(sizes) {
                child.1 = *size;
            }
            return true;
        }
        children
            .iter_mut()
            .any(|(child, _)| child.resize(at, sizes))
    }

    /// The arrangement, without the shares.
    ///
    /// What a view of the layout actually depends on: dragging a divider changes the shares on
    /// every move and the arrangement on none of them, so a view rebuilt on this is a view that
    /// survives the drag that is rebuilding it.
    #[must_use]
    pub fn shape(&self) -> Shape {
        match self {
            Self::Leaf(window) => Shape::Leaf(*window),
            Self::Split { axis, children } => Shape::Split {
                axis: *axis,
                children: children.iter().map(|(child, _)| child.shape()).collect(),
            },
        }
    }

    /// The window after `at` in the walking order, wrapping.
    pub fn next_after(&self, at: WindowId) -> Option<WindowId> {
        let windows = self.windows();
        let index = windows.iter().position(|id| *id == at)?;
        windows.get((index + 1) % windows.len()).copied()
    }

    /// The window before `at` in the walking order, wrapping.
    pub fn previous_before(&self, at: WindowId) -> Option<WindowId> {
        let windows = self.windows();
        let index = windows.iter().position(|id| *id == at)?;
        let count = windows.len();
        windows.get((index + count - 1) % count).copied()
    }
}

#[cfg(test)]
mod tests {
    use slotmap::SlotMap;

    use super::{Axis, Direction, Layout, WindowId};

    fn ids(count: usize) -> Vec<WindowId> {
        let mut map: SlotMap<WindowId, ()> = SlotMap::with_key();
        (0..count).map(|_| map.insert(())).collect()
    }

    #[test]
    fn one_window_is_one_leaf() {
        let id = ids(1)[0];
        let layout = Layout::Leaf(id);
        assert!(layout.is_single());
        assert_eq!(layout.windows(), vec![id]);
        assert_eq!(layout.len(), 1);
    }

    #[test]
    fn splitting_a_leaf_halves_it() {
        let id = ids(2);
        let mut layout = Layout::Leaf(id[0]);
        assert!(layout.split(id[0], Axis::Horizontal, id[1]));
        assert_eq!(layout.windows(), vec![id[0], id[1]]);
        match &layout {
            Layout::Split { children, .. } => {
                assert_eq!(children[0].1, 50.0);
                assert_eq!(children[1].1, 50.0);
            }
            Layout::Leaf(_) => panic!("it did not split"),
        }
    }

    #[test]
    fn splitting_along_the_same_axis_joins_the_row() {
        // Three vertical splits are one row of three. Nested, their sizes would not add up to
        // what the user sees and dragging one handle would move the wrong edge.
        let id = ids(3);
        let mut layout = Layout::Leaf(id[0]);
        layout.split(id[0], Axis::Horizontal, id[1]);
        layout.split(id[1], Axis::Horizontal, id[2]);
        match &layout {
            Layout::Split { children, axis } => {
                assert_eq!(*axis, Axis::Horizontal);
                assert_eq!(children.len(), 3);
                let total: f64 = children.iter().map(|(_, size)| size).sum();
                assert!((total - 100.0).abs() < 0.001, "{total}");
            }
            Layout::Leaf(_) => panic!("it did not split"),
        }
    }

    #[test]
    fn splitting_across_the_axis_nests() {
        let id = ids(3);
        let mut layout = Layout::Leaf(id[0]);
        layout.split(id[0], Axis::Horizontal, id[1]);
        layout.split(id[1], Axis::Vertical, id[2]);
        assert_eq!(layout.windows(), vec![id[0], id[1], id[2]]);
        match &layout {
            Layout::Split { children, .. } => {
                assert_eq!(children.len(), 2);
                assert!(matches!(children[1].0, Layout::Split { .. }));
            }
            Layout::Leaf(_) => panic!("it did not split"),
        }
    }

    #[test]
    fn closing_one_of_two_leaves_the_other_whole() {
        let id = ids(2);
        let mut layout = Layout::Leaf(id[0]);
        layout.split(id[0], Axis::Horizontal, id[1]);
        assert!(layout.close(id[1]));
        assert_eq!(layout, Layout::Leaf(id[0]));
    }

    #[test]
    fn closing_gives_the_space_to_a_neighbour() {
        let id = ids(3);
        let mut layout = Layout::Leaf(id[0]);
        layout.split(id[0], Axis::Horizontal, id[1]);
        layout.split(id[1], Axis::Horizontal, id[2]);
        layout.close(id[2]);
        match &layout {
            Layout::Split { children, .. } => {
                let total: f64 = children.iter().map(|(_, size)| size).sum();
                assert!((total - 100.0).abs() < 0.001, "{total}");
            }
            Layout::Leaf(_) => panic!("two windows are still a split"),
        }
    }

    #[test]
    fn the_last_window_does_not_close() {
        // Something has to be on screen.
        let id = ids(1)[0];
        let mut layout = Layout::Leaf(id);
        assert!(!layout.close(id));
        assert_eq!(layout, Layout::Leaf(id));
    }

    #[test]
    fn walking_wraps_both_ways() {
        let id = ids(3);
        let mut layout = Layout::Leaf(id[0]);
        layout.split(id[0], Axis::Horizontal, id[1]);
        layout.split(id[1], Axis::Horizontal, id[2]);

        assert_eq!(layout.next_after(id[0]), Some(id[1]));
        assert_eq!(layout.next_after(id[2]), Some(id[0]));
        assert_eq!(layout.previous_before(id[0]), Some(id[2]));
    }

    #[test]
    fn a_dragged_handle_writes_the_sizes_of_its_own_split() {
        let id = ids(2);
        let mut layout = Layout::Leaf(id[0]);
        layout.split(id[0], Axis::Horizontal, id[1]);
        assert!(layout.resize(id[0], &[30.0, 70.0]));
        match &layout {
            Layout::Split { children, .. } => {
                assert_eq!(children[0].1, 30.0);
                assert_eq!(children[1].1, 70.0);
            }
            Layout::Leaf(_) => panic!("it did not split"),
        }
    }

    #[test]
    fn a_side_by_side_split_is_crossed_left_and_right() {
        let id = ids(2);
        let mut layout = Layout::Leaf(id[0]);
        layout.split(id[0], Axis::Horizontal, id[1]);

        assert_eq!(layout.neighbour(id[0], Direction::Right), Some(id[1]));
        assert_eq!(layout.neighbour(id[1], Direction::Left), Some(id[0]));
    }

    #[test]
    fn a_split_the_other_way_is_not_crossed() {
        let id = ids(2);
        let mut layout = Layout::Leaf(id[0]);
        layout.split(id[0], Axis::Horizontal, id[1]);

        // Side by side: there is nothing above or below either of them.
        assert_eq!(layout.neighbour(id[0], Direction::Down), None);
        assert_eq!(layout.neighbour(id[1], Direction::Up), None);
    }

    #[test]
    fn the_edge_of_the_layout_has_no_neighbour() {
        let id = ids(2);
        let mut layout = Layout::Leaf(id[0]);
        layout.split(id[0], Axis::Horizontal, id[1]);

        assert_eq!(layout.neighbour(id[0], Direction::Left), None);
        assert_eq!(layout.neighbour(id[1], Direction::Right), None);
    }

    #[test]
    fn crossing_out_of_a_nested_split_climbs_to_find_one() {
        // Left | (top over bottom). From either of the two on the right, `h` is the one on the
        // left — the split that divides that way is two levels up.
        let id = ids(3);
        let mut layout = Layout::Leaf(id[0]);
        layout.split(id[0], Axis::Horizontal, id[1]);
        layout.split(id[1], Axis::Vertical, id[2]);

        assert_eq!(layout.neighbour(id[1], Direction::Left), Some(id[0]));
        assert_eq!(layout.neighbour(id[2], Direction::Left), Some(id[0]));
        assert_eq!(layout.neighbour(id[1], Direction::Down), Some(id[2]));
        assert_eq!(layout.neighbour(id[2], Direction::Up), Some(id[1]));
    }

    #[test]
    fn crossing_into_a_split_enters_at_its_near_edge() {
        // (top over bottom) | right. Going left from the right-hand one enters the left column at
        // its first window rather than at whichever happens to be last.
        let id = ids(3);
        let mut layout = Layout::Leaf(id[0]);
        layout.split(id[0], Axis::Horizontal, id[1]);
        layout.split(id[0], Axis::Vertical, id[2]);

        assert_eq!(layout.neighbour(id[1], Direction::Left), Some(id[0]));
    }

    #[test]
    fn a_window_that_is_not_in_the_layout_has_no_neighbour() {
        let id = ids(2);
        let layout = Layout::Leaf(id[0]);
        assert_eq!(layout.neighbour(id[1], Direction::Right), None);
    }

    #[test]
    fn a_direction_is_named_the_way_the_keymap_writes_it() {
        assert_eq!(Direction::named("left"), Some(Direction::Left));
        assert_eq!(Direction::named("down"), Some(Direction::Down));
        assert_eq!(Direction::named("sideways"), None);
        assert_eq!(Direction::Left.axis(), Axis::Horizontal);
        assert_eq!(Direction::Up.axis(), Axis::Vertical);
        assert!(Direction::Right.forward());
        assert!(!Direction::Up.forward());
    }
}
