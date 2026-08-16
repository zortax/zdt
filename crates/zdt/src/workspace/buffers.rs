//! Opening buffers, showing them, and closing them.

use super::*;

impl Workspace {
    // ---- Buffers ---------------------------------------------------------------------------

    /// Adds a buffer over `document` and shows it in the focused window.
    ///
    /// A file already open is shown, and never opened twice. That makes `<Leader>ff` onto
    /// something already on the buffer line a jump.
    pub fn open_document(
        &self,
        path: Option<PathBuf>,
        document: zgui_editor::Document,
    ) -> BufferId {
        if let Some(path) = path.as_deref()
            && let Some(existing) = self.find_path(path)
        {
            self.show(existing);
            return existing;
        }

        let id = self
            .owned(|| {
                self.inner.buffers.try_update(|buffers| {
                    buffers.insert_with_key(|id| Buffer::text(id, path, document))
                })
            })
            .expect("the buffer map is writable");
        self.inner.order.update(|order| order.push(id));
        self.show(id);
        id
    }

    /// Adds a buffer that is not text, such as a terminal, and shows it.
    pub fn open_buffer(&self, make: impl FnOnce(BufferId) -> Buffer) -> BufferId {
        let id = self
            .owned(|| {
                self.inner
                    .buffers
                    .try_update(|buffers| buffers.insert_with_key(make))
            })
            .expect("the buffer map is writable");
        self.inner.order.update(|order| order.push(id));
        self.show(id);
        id
    }

    /// Puts `buffer` back in place of the one with its identity.
    ///
    /// For the fields read once the file has arrived: how it is spelled, and what it breaks its
    /// lines with. The buffer could know neither when it was made.
    pub fn replace_buffer(&self, buffer: Buffer) {
        let id = buffer.id;
        self.inner.buffers.update(|buffers| {
            if let Some(held) = buffers.get_mut(id) {
                *held = buffer;
            }
        });
    }

    /// Shows `id` in the focused window.
    pub fn show(&self, id: BufferId) {
        self.show_in(self.focused_untracked(), id);
    }

    /// Shows `id` in `window`.
    ///
    /// The buffer it was showing becomes the alternate, which is what `<Leader>bp` goes back to,
    /// and stays mounted so that going back is instant.
    pub fn show_in(&self, window: WindowId, id: BufferId) {
        let previous = self.buffer_in_untracked(window);
        if previous == Some(id) {
            return;
        }
        if let Some(previous) = previous {
            self.inner.alternate.set(Some(previous));
        }
        // Every file shown is a file recently opened, whichever way it was reached: the picker,
        // the tree, the buffer line or the command line.
        if let Some(path) = self.buffer_untracked(id).and_then(|buffer| buffer.path) {
            self.remember(&path);
        }
        self.inner.windows.update(|windows| {
            let Some(state) = windows.get_mut(window) else {
                return;
            };
            state.current = Some(id);
            state.mounted.retain(|held| *held != id);
            state.mounted.insert(0, id);
            state.mounted.truncate(MOUNTED_PER_WINDOW);
        });
    }

    /// Opens a terminal buffer called `name`, and answers it.
    ///
    /// The program itself is [`crate::terminals`]'s business; what this makes is the buffer it
    /// will be drawn in, so that a terminal is on the buffer line like everything else.
    /// A floating terminal is `listed = false`. The key that toggles it is the only way to reach
    /// it, which makes it a scratch terminal and never another tab to close.
    pub fn open_terminal(&self, name: &str, listed: bool) -> BufferId {
        let id = self
            .owned(|| {
                self.inner
                    .buffers
                    .try_update(|buffers| buffers.insert_with_key(|id| Buffer::terminal(id, name)))
            })
            .expect("the buffer map is writable");
        if listed {
            self.inner.order.update(|order| order.push(id));
            self.show(id);
        }
        id
    }

    /// Opens a panel buffer of `kind`, or shows the one that is already open.
    ///
    /// One at a time: a second settings tab would be a second copy of the same page, and closing
    /// the first would leave somebody wondering which one they had been changing.
    pub fn open_panel(&self, kind: BufferKind) -> BufferId {
        let wanted = std::mem::discriminant(&kind);
        let existing = self.inner.order.with_untracked(|order| {
            order.iter().copied().find(|id| {
                self.buffer_untracked(*id)
                    .is_some_and(|buffer| std::mem::discriminant(&buffer.kind) == wanted)
            })
        });
        if let Some(id) = existing {
            self.show(id);
            return id;
        }

        let id = self
            .owned(|| {
                self.inner
                    .buffers
                    .try_update(|buffers| buffers.insert_with_key(|id| Buffer::panel(id, kind)))
            })
            .expect("the buffer map is writable");
        self.inner.order.update(|order| order.push(id));
        self.show(id);
        id
    }

    /// Puts the title a program asked for on its buffer.
    pub fn rename_terminal(&self, id: BufferId, title: Option<String>) {
        let Some(buffer) = self.buffer_untracked(id) else {
            return;
        };
        let crate::workspace::BufferKind::Terminal { title: held } = &buffer.kind else {
            return;
        };
        // A program that clears its title leaves the one it had: an empty tab says less than a
        // stale one.
        if let Some(title) = title.filter(|title| !title.is_empty())
            && held.get_untracked().as_deref() != Some(title.as_str())
        {
            held.set(Some(title));
        }
    }

    /// Closes `id`, showing something else wherever it was.
    ///
    /// The buffer's text goes with it: a closed buffer is closed, and its undo history is not
    /// something an editor keeps for a file nobody has open. Answers whether there was one.
    pub fn close_buffer(&self, id: BufferId) -> bool {
        // What a window showing this should show instead: the next buffer along, or nothing when
        // that was the last one. Nothing is a real answer, and an empty window says so.
        let order = self.inner.order.get_untracked();
        let at = order.iter().position(|held| *held == id);
        let replacement = at.and_then(|at| {
            order
                .get(at + 1)
                .or_else(|| at.checked_sub(1).and_then(|before| order.get(before)))
                .copied()
        });

        let existed = self
            .inner
            .buffers
            .try_update(|buffers| buffers.remove(id).is_some())
            .unwrap_or(false);
        if !existed {
            return false;
        }

        self.inner
            .order
            .update(|order| order.retain(|held| *held != id));
        self.inner.alternate.update(|alternate| {
            if *alternate == Some(id) {
                *alternate = None;
            }
        });
        self.inner.windows.update(|windows| {
            for state in windows.values_mut() {
                state.mounted.retain(|held| *held != id);
                if state.current == Some(id) {
                    state.current = replacement;
                    if let Some(replacement) = replacement
                        && !state.mounted.contains(&replacement)
                    {
                        state.mounted.insert(0, replacement);
                    }
                }
            }
        });
        true
    }

    /// Shows the buffer `offset` places along the buffer line from the current one, wrapping.
    pub fn cycle_buffer(&self, offset: isize) {
        let order = self.inner.order.get_untracked();
        if order.len() < 2 {
            return;
        }
        let Some(current) = self.buffer_in_untracked(self.focused_untracked()) else {
            return;
        };
        let Some(index) = order.iter().position(|held| *held == current) else {
            return;
        };
        let count = order.len() as isize;
        let next = (index as isize + offset).rem_euclid(count) as usize;
        self.show(order[next]);
    }

    /// Goes back to the buffer that was shown before this one.
    pub fn show_alternate(&self) {
        if let Some(alternate) = self.inner.alternate.get_untracked() {
            self.show(alternate);
        }
    }

    /// Moves the current buffer `offset` places along the buffer line.
    pub fn move_buffer(&self, offset: isize) {
        let Some(current) = self.buffer_in_untracked(self.focused_untracked()) else {
            return;
        };
        self.inner.order.update(|order| {
            let Some(index) = order.iter().position(|held| *held == current) else {
                return;
            };
            let count = order.len() as isize;
            let target = (index as isize + offset).clamp(0, count - 1) as usize;
            let id = order.remove(index);
            order.insert(target, id);
        });
    }
}
