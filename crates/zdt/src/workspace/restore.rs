//! Putting the splits and the buffer line back the way a session left them.
//!
//! Everything here writes the maps directly, because a restore is not a sequence of the things
//! somebody did: replaying forty `:vsplit`s to arrive at a layout would arrive at a *different*
//! layout, because splitting divides whatever is focused at the time.
//!
//! `WindowId` and `BufferId` are slotmap keys and mean nothing across a restart, so what comes
//! back from disk is indices and the mapping is built here.

use super::*;

/// One split, as a session wrote it down.
#[derive(Clone, Debug, Default)]
pub struct Restored {
    /// Which buffer it was showing, by its place in the restored buffer list.
    pub current: Option<usize>,
    /// How much larger its text was than the setting says.
    pub font_step: i32,
    /// Which buffers it showed rich, by their places in the restored buffer list.
    pub rich: Vec<usize>,
}

impl Workspace {
    /// Rebuilds the splits, and answers the identity each index was given.
    ///
    /// `layout` is a tree over the same indices `windows` is in. A tree naming a split that is
    /// not there, or naming none at all, falls back to one window showing nothing — a workspace
    /// with no window has nowhere to put the next buffer.
    ///
    /// `buffers` maps each index in a [`Restored::current`] to the buffer that index became.
    pub fn restore_layout(
        &self,
        windows: &[Restored],
        layout: impl Fn(&[WindowId]) -> Option<Layout>,
        buffers: &[BufferId],
        focused: usize,
    ) -> Vec<WindowId> {
        if windows.is_empty() {
            return self.windows();
        }

        let made: Vec<WindowId> = self
            .inner
            .windows
            .try_update(|held| {
                let fresh: Vec<WindowId> = windows
                    .iter()
                    .map(|window| {
                        let current = window.current.and_then(|at| buffers.get(at).copied());
                        held.insert(WindowState {
                            current,
                            // Only what is showing. The rest of the warm cache refills itself on
                            // the first switch, and restoring it would force every buffer's
                            // document to exist before the first frame.
                            mounted: current.into_iter().collect(),
                            font_step: window.font_step,
                            rich: window
                                .rich
                                .iter()
                                .filter_map(|at| buffers.get(*at).copied())
                                .collect(),
                        })
                    })
                    .collect();
                // The old windows go, so the scratch one the workspace was made with does not
                // linger beside the restored ones.
                let keep: Vec<WindowId> = fresh.clone();
                held.retain(|id, _| keep.contains(&id));
                fresh
            })
            .unwrap_or_default();

        let Some(tree) = layout(&made) else {
            return self.windows();
        };
        self.inner.layout.set(tree);
        let focused = made.get(focused).or_else(|| made.first()).copied();
        if let Some(focused) = focused {
            self.inner.focus.enter_window(focused);
        }
        made
    }

    /// Puts the buffer line back in the order a session wrote it down in.
    ///
    /// Anything the order does not name keeps its place at the end, so a buffer opened while the
    /// restore was still running is not lost.
    pub fn restore_order(&self, wanted: &[BufferId]) {
        self.inner.order.update(|order| {
            let mut sorted: Vec<BufferId> = wanted
                .iter()
                .copied()
                .filter(|id| order.contains(id))
                .collect();
            for id in order.iter() {
                if !sorted.contains(id) {
                    sorted.push(*id);
                }
            }
            *order = sorted;
        });
    }

    /// Says which buffer `<Leader>bp` goes back to.
    pub fn set_alternate(&self, id: Option<BufferId>) {
        self.inner.alternate.set(id);
    }

    /// Puts the recently-opened list back, which the picker of old files reads.
    pub fn restore_recent(&self, paths: Vec<std::path::PathBuf>) {
        *self.inner.recent.borrow_mut() = paths;
    }

    /// Closes the buffer a workspace is made holding, once a session has put real ones in.
    ///
    /// A restored session that also showed an empty scratch buffer would show one nobody asked
    /// for. Does nothing when it is the only buffer there is.
    pub fn drop_scratch(&self) {
        let order = self.order_untracked();
        if order.len() < 2 {
            return;
        }
        let scratch = order.iter().copied().find(|id| {
            self.buffer_untracked(*id).is_some_and(|buffer| {
                buffer.path.is_none()
                    && matches!(buffer.kind, BufferKind::Text { .. })
                    && !buffer.dirty.get_untracked()
            })
        });
        if let Some(scratch) = scratch {
            self.close_buffer(scratch);
        }
    }
}
