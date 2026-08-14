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

impl Layout {
    /// Every window in the tree, left to right and top to bottom.
    ///
    /// The order `<C-w>w` walks and the order a session writes them down in.
    pub fn windows(&self) -> Vec<WindowId> {
        let mut found = Vec::new();
        self.collect(&mut found);
        found
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

    use super::{Axis, Layout, WindowId};

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
}
