//! The lists that are gathered once and never grow.

use super::*;

impl Picker {
    // ---- The standing lists ------------------------------------------------------------------

    /// The open buffers, with the one being edited last. Somebody is switching away from it, so
    /// it is the least likely thing they are switching to.
    pub(super) fn buffers(&self) -> Vec<Row> {
        let current = self
            .inner
            .workspace
            .current_buffer()
            .map(|buffer| buffer.id);
        let mut rows: Vec<Row> = Vec::new();
        for id in self.inner.workspace.order() {
            let Some(buffer) = self.inner.workspace.buffer_untracked(id) else {
                continue;
            };
            let label = match &buffer.path {
                Some(path) => self.inner.workspace.project().relative(path).into_owned(),
                None => "[no name]".to_owned(),
            };
            let kind = buffer
                .path
                .as_deref()
                .map(zdt_core::language::of)
                .unwrap_or(zdt_core::language::UNKNOWN);
            let row = Row {
                label,
                detail: if buffer.is_dirty() {
                    "modified".to_owned()
                } else {
                    String::new()
                },
                matched: Vec::new(),
                glyph: Some(kind.glyph),
                tint: Some(kind.tint),
                icon: None,
                target: Target::Buffer(id),
            };
            if Some(id) == current {
                rows.push(row);
            } else {
                rows.insert(0, row);
            }
        }
        rows.reverse();
        rows
    }

    /// The files opened this session, the most recent first, leaving out the ones still open.
    ///
    /// A file that is open is one keystroke away on the buffer line; a recent-files list that
    /// repeated it would be spending its rows on the answer somebody already has.
    pub(super) fn recent(&self) -> Vec<Row> {
        let root = self.inner.workspace.project().root().to_path_buf();
        let open: Vec<std::path::PathBuf> = self
            .inner
            .workspace
            .order()
            .into_iter()
            .filter_map(|id| self.inner.workspace.buffer_untracked(id))
            .filter_map(|buffer| buffer.path)
            .collect();

        self.inner
            .workspace
            .recent()
            .into_iter()
            .filter(|path| !open.contains(path))
            .map(|path| {
                let shown = self.inner.workspace.project().relative(&path).into_owned();
                Row::file(shown, &root, None)
            })
            .collect()
    }

    /// What is in each register, as one row each.
    pub(super) fn registers(&self) -> Vec<Row> {
        let Some(vim) = zgui::reactive::use_local_context::<crate::vim::Vim>() else {
            return Vec::new();
        };
        vim.registers()
            .into_iter()
            .map(|(name, text)| {
                // One line of it: a register holding forty lines is still one row here, and the
                // first line is the part that says which one it is.
                let first = text.lines().next().unwrap_or("").trim_end();
                Row::plain(format!("\"{name}"), Target::Nothing).with_detail(first.to_owned())
            })
            .collect()
    }

    /// Where each mark is, as the line it sits on.
    pub(super) fn marks(&self) -> Vec<Row> {
        let Some(vim) = zgui::reactive::use_local_context::<crate::vim::Vim>() else {
            return Vec::new();
        };
        let Some(handle) = self.inner.workspace.current_handle() else {
            return Vec::new();
        };

        handle.query(|snapshot| {
            let rope = snapshot.rope();
            vim.marks()
                .into_iter()
                .map(|(name, place)| {
                    let byte = place.byte.min(rope.len_bytes());
                    let line = rope.byte_to_line(byte);
                    let text = rope
                        .line(line)
                        .to_string()
                        .trim_end_matches(['\n', '\r'])
                        .trim_start()
                        .to_owned();
                    Row::plain(format!("'{name}"), Target::Line(line as u64 + 1))
                        .with_detail(format!("{}  {text}", line + 1))
                })
                .collect()
        })
    }

    /// The files git is tracking, which is the file list minus everything untracked.
    pub(super) fn gather_git(&self, query: &str) {
        let generation = self.inner.generation.get();
        let root = self.inner.workspace.project().root().to_path_buf();
        let picker = self.clone();
        let query = query.to_owned();

        self.inner.working.set(true);
        zdt_view::detached(async move {
            let listed = {
                let root = root.clone();
                zgui::task::blocking(move || git_files(&root)).await
            };
            if picker.inner.generation.get() != generation {
                return;
            }
            picker.inner.working.set(false);
            let rows = listed
                .into_iter()
                .map(|path| Row::file(path, &root, None))
                .collect();
            picker.stand(rows, &query);
        });
    }

    /// The lines of the buffer being edited.
    pub(super) fn lines(&self) -> Vec<Row> {
        let Some(handle) = self.inner.workspace.current_handle() else {
            return Vec::new();
        };
        handle.query(|snapshot| {
            let rope = snapshot.rope();
            rope.lines()
                .enumerate()
                .map(|(index, line)| {
                    let text = line.to_string();
                    Row::plain(
                        text.trim_end_matches(['\n', '\r']).to_owned(),
                        Target::Line(index as u64 + 1),
                    )
                    .with_detail(format!("{}", index + 1))
                })
                .collect()
        })
    }

    /// The themes there are, the built-in ones and whatever is in the configuration directory.
    pub(super) fn themes(&self) -> Vec<Row> {
        let directory = self.inner.settings.paths().map(|paths| paths.themes());
        zdt_core::theme::theme_names(directory.as_deref())
            .into_iter()
            .map(|name| Row::plain(name.clone(), Target::Theme(name)))
            .collect()
    }

    /// Everything the keymap can do.
    ///
    /// As commands, it is one row per description, so the same thing bound twice reads once. As
    /// keys, it is one row per binding, because which key does it is the question being asked.
    pub(super) fn bindings(&self, by_key: bool) -> Vec<Row> {
        let Some(vim) = zgui::reactive::use_local_context::<crate::vim::Vim>() else {
            return Vec::new();
        };
        let mut rows: Vec<Row> = Vec::new();
        let mut seen: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();

        for bound in vim.bindings() {
            let described = if bound.description.is_empty() {
                bound
                    .actions
                    .first()
                    .map_or(String::new(), |action| action.name.replace(['.', '_'], " "))
            } else {
                bound.description.clone()
            };
            let Some(action) = bound.actions.first().cloned() else {
                continue;
            };

            if by_key {
                rows.push(
                    Row::plain(bound.keys.clone(), Target::Action(action)).with_detail(described),
                );
            } else if seen.insert(described.clone()) {
                rows.push(Row::plain(described, Target::Action(action)).with_detail(bound.keys));
            }
        }
        rows.sort_by(|left, right| left.label.cmp(&right.label));
        rows
    }
}
