//! The mounted editors, and which of them has the keyboard.

use super::*;

/// How a buffer's kind holds its rich form.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Held {
    /// The rich form is the only form.
    Only,
    /// Rich by default; the window records the splits shown as source.
    Rich,
    /// Source by default; the window records the splits shown rich.
    Source,
}

/// Which list a toggle writes.
#[derive(Clone, Copy)]
enum Departures {
    Rich,
    Plain,
}

impl Workspace {
    // ---- The editors themselves -------------------------------------------------------------

    /// Remembers the editor showing `buffer` in `window`.
    pub fn register_handle(
        &self,
        window: WindowId,
        buffer: BufferId,
        handle: zgui_editor::EditorHandle,
    ) {
        self.inner
            .handles
            .borrow_mut()
            .insert((window, buffer), handle);
        self.inner
            .mounted
            .update(|revision| *revision = revision.wrapping_add(1));
    }

    /// A number that changes whenever an editor arrives or goes away. Tracked.
    ///
    /// What the focus effect reads so that an editor mounting after its window was focused still
    /// ends up with the keyboard.
    #[must_use]
    pub fn mounted_revision(&self) -> u64 {
        self.inner.mounted.get()
    }

    /// Forgets it, which a view does as it unmounts.
    ///
    /// `handle` is the editor that is going away, and the entry is dropped only if it is still
    /// the one filed here. A pane rebuilt in place mounts its new editor *before* the old one is
    /// cleaned up, which is what splitting does to every pane the new layout re-creates. So the
    /// two orders overlap: register the new, then forget the old. Forgetting by key alone deletes
    /// the registration the new editor had just made, and the window then has an editor on the
    /// screen and no handle to it. Nothing draws differently. What breaks is everything that asks
    /// the workspace for the editor of a window, and `<C-k>` first among them.
    pub fn forget_handle(
        &self,
        window: WindowId,
        buffer: BufferId,
        handle: &zgui_editor::EditorHandle,
    ) {
        let mut handles = self.inner.handles.borrow_mut();
        if handles.get(&(window, buffer)) != Some(handle) {
            return;
        }
        handles.remove(&(window, buffer));
        drop(handles);
        self.inner
            .mounted
            .update(|revision| *revision = revision.wrapping_add(1));
    }

    /// The editor showing `buffer` in `window`, when one is mounted.
    pub fn handle_for(
        &self,
        window: WindowId,
        buffer: BufferId,
    ) -> Option<zgui_editor::EditorHandle> {
        self.inner.handles.borrow().get(&(window, buffer)).cloned()
    }

    /// The editor the keyboard is in, when it is in one.
    ///
    /// What every action that edits text goes through, and the one place that answers "the
    /// editor". Everything else says which window it means.
    pub fn current_handle(&self) -> Option<zgui_editor::EditorHandle> {
        let window = self.focused_untracked();
        let buffer = self.buffer_in_untracked(window)?;
        self.handle_for(window, buffer)
    }

    /// Every file that has been open in this session, the most recent first.
    ///
    /// Kept, and never derived from the open buffers. The point of a recent-files list is the
    /// ones that are *not* open any more.
    #[must_use]
    pub fn recent(&self) -> Vec<PathBuf> {
        self.inner.recent.borrow().clone()
    }

    /// Remembers `path` as the most recently opened.
    pub fn remember(&self, path: &Path) {
        let mut recent = self.inner.recent.borrow_mut();
        recent.retain(|held| held != path);
        recent.insert(0, path.to_path_buf());
        // A session's worth, which is as much as anybody scrolls: this is not a history file.
        recent.truncate(200);
    }

    /// Moves the keyboard to the window `direction` of the focused one.
    ///
    /// Answers whether there was one. Nothing that way is not an error: `<C-w>h` in the leftmost
    /// window is a key that does nothing, the same as in vim.
    /// Makes this window's text `step` pixels larger, or puts it back when `step` is zero.
    pub fn zoom(&self, window: WindowId, step: i32) {
        self.inner.windows.update(|windows| {
            let Some(state) = windows.get_mut(window) else {
                return;
            };
            state.font_step = if step == 0 {
                0
            } else {
                // Bounded either way: text of no pixels draws nothing, and text the size of the
                // window leaves no room for any.
                (state.font_step + step).clamp(-6, 24)
            };
        });
    }

    /// How much larger this window's text is than the setting. Tracked.
    #[must_use]
    pub fn font_step(&self, window: WindowId) -> i32 {
        self.inner
            .windows
            .with(|windows| windows.get(window).map_or(0, |state| state.font_step))
    }

    /// Subscribes to every window's state: what it shows, its text size, its rich buffers.
    ///
    /// For a watcher that writes the session down: the layout and the order have signals of
    /// their own, and what is *inside* a window changes without either of them moving.
    pub fn track_windows(&self) {
        self.inner.windows.with(|_| ());
    }

    /// Whether `window` shows `buffer` in its rich form. Tracked.
    ///
    /// The default comes from the buffer's kind, and the window records only departures from
    /// it: `rich` for a kind that starts in the source, `plain` for one that starts rich. The
    /// kind is fixed when the buffer is made, so asking for it subscribes to nothing.
    #[must_use]
    pub fn is_rich(&self, window: WindowId, buffer: BufferId) -> bool {
        match self.rich_default(buffer) {
            Some(Held::Only) => true,
            Some(Held::Rich) => self.inner.windows.with(|windows| {
                windows
                    .get(window)
                    .is_some_and(|state| !state.plain.contains(&buffer))
            }),
            Some(Held::Source) => self.inner.windows.with(|windows| {
                windows
                    .get(window)
                    .is_some_and(|state| state.rich.contains(&buffer))
            }),
            None => false,
        }
    }

    /// The same, without subscribing.
    #[must_use]
    pub fn is_rich_untracked(&self, window: WindowId, buffer: BufferId) -> bool {
        match self.rich_default(buffer) {
            Some(Held::Only) => true,
            Some(Held::Rich) => self.inner.windows.with_untracked(|windows| {
                windows
                    .get(window)
                    .is_some_and(|state| !state.plain.contains(&buffer))
            }),
            Some(Held::Source) => self.inner.windows.with_untracked(|windows| {
                windows
                    .get(window)
                    .is_some_and(|state| state.rich.contains(&buffer))
            }),
            None => false,
        }
    }

    /// How the buffer's kind holds its rich form, when it has one.
    fn rich_default(&self, buffer: BufferId) -> Option<Held> {
        let entry = self.buffer_untracked(buffer)?;
        let kind = crate::rich::RichKind::of(&entry)?;
        Some(if !kind.has_source() {
            Held::Only
        } else if kind.starts_in() == crate::rich::Presentation::Rich {
            Held::Rich
        } else {
            Held::Source
        })
    }

    /// Flips which form `window` shows `buffer` in.
    ///
    /// Does nothing for a buffer with only one form.
    pub fn toggle_rich(&self, window: WindowId, buffer: BufferId) {
        let departures = match self.rich_default(buffer) {
            Some(Held::Rich) => Departures::Plain,
            Some(Held::Source) => Departures::Rich,
            _ => return,
        };
        self.inner.windows.update(|windows| {
            let Some(state) = windows.get_mut(window) else {
                return;
            };
            let held = match departures {
                Departures::Rich => &mut state.rich,
                Departures::Plain => &mut state.plain,
            };
            if held.contains(&buffer) {
                held.retain(|kept| *kept != buffer);
            } else {
                held.push(buffer);
            }
        });
    }

    pub fn focus_direction(&self, direction: crate::workspace::Direction) -> bool {
        let from = self.focused_untracked();
        let Some(next) = self
            .inner
            .layout
            .with_untracked(|layout| layout.neighbour(from, direction))
        else {
            return false;
        };
        self.focus_window(next);
        true
    }

    /// Gives the keyboard straight to the current editor.
    ///
    /// For the one place that means *the editor* rather than "wherever the keyboard belongs": a
    /// leap ends by putting the caret back where it can be typed at. Everything else asks the model
    /// through [`crate::focus::Focusing::reproject`], which has an answer for a window holding a
    /// terminal or a panel too.
    pub fn focus_editor(&self) {
        if let Some(handle) = self.current_handle() {
            handle.focus();
        }
    }
}
