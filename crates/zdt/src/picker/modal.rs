//! Opening the modal, closing it, and moving the caret.

use super::*;

impl Picker {
    // ---- Opening and closing -----------------------------------------------------------------

    /// Opens `source`, gathering whatever it needs.
    pub fn open(&self, source: Source) {
        self.stop();
        // What to put back if this is given up on. Taken before anything is shown, because a
        // preview overwrites it.
        let held = matches!(source, Source::Themes).then(|| {
            self.inner
                .settings
                .with_untracked(|config| config.ui.theme.clone())
        });
        *self.inner.restore.borrow_mut() = held.clone();
        let start = source.start();
        self.inner.at.set(0);
        self.inner.rows.set(Vec::new());
        self.inner.counts.set((0, 0));
        self.inner.query.set(start.clone());
        self.inner.source.set(Some(source.clone()));
        // The theme picker opens on the theme in force rather than on the first name in the list.
        // Opening it is not choosing anything, and a picker that previewed something else the
        // moment it appeared would change the colours of the very screen somebody opened it to
        // look at.
        *self.inner.land.borrow_mut() = held;
        self.gather(&source, &start);
    }

    /// Closes it, putting back anything a preview changed.
    ///
    /// What `<Esc>` does. Choosing a row calls [`Picker::activate`], which keeps what it showed.
    pub fn close(&self) {
        if self.inner.source.with_untracked(Option::is_none) {
            return;
        }
        if let Some(held) = self.inner.restore.borrow_mut().take()
            && self
                .inner
                .settings
                .with_untracked(|config| config.ui.theme != held)
        {
            self.inner
                .settings
                .update(|config| config.ui.theme.clone_from(&held));
        }
        self.stop();
        self.inner.source.set(None);
        self.inner.rows.set(Vec::new());
        self.inner.candidates.borrow_mut().clear();
        *self.inner.ranker.borrow_mut() = None;
        self.inner.workspace.focus_editor();
    }

    /// Stops every worker this picker has running, without closing it.
    pub(super) fn stop(&self) {
        self.inner.generation.set(self.inner.generation.get() + 1);
        *self.inner.polling.borrow_mut() = None;
        *self.inner.pending.borrow_mut() = None;
        if let Some(cancel) = self.inner.cancel.borrow_mut().take() {
            cancel.stop();
        }
        self.inner.working.set(false);
    }

    // ---- Moving about ------------------------------------------------------------------------

    /// Moves the caret by `offset` rows, wrapping the way a picker does.
    pub fn move_by(&self, offset: isize) {
        let count = self.inner.rows.with_untracked(Vec::len);
        if count == 0 {
            return;
        }
        let at = self.inner.at.get_untracked() as isize + offset;
        let wrapped = at.rem_euclid(count as isize) as usize;
        self.inner.at.set(wrapped);
        self.preview_row();
    }

    /// Puts the caret on `at`.
    pub fn go_to(&self, at: usize) {
        let count = self.inner.rows.with_untracked(Vec::len);
        if count > 0 {
            self.inner.at.set(at.min(count - 1));
            self.preview_row();
        }
    }

    /// Puts the caret on the row [`Picker::open`] asked to land on, when the ranking has one.
    ///
    /// Called once per opening, from the first ranking. A name that matched nothing — a theme that
    /// has since been deleted from the configuration directory — leaves the caret at the top.
    pub(super) fn land_caret(&self) {
        let Some(wanted) = self.inner.land.borrow_mut().take() else {
            return;
        };
        let at = self.inner.rows.with_untracked(|rows| {
            rows.iter()
                .position(|row| matches!(&row.target, Target::Theme(name) if *name == wanted))
        });
        if let Some(at) = at {
            self.inner.at.set(at);
        }
    }

    /// Shows what the row under the caret would do, for the sources where showing *is* the answer.
    ///
    /// Only the themes so far. A theme read as a name is a name; a theme applied is a theme, and
    /// nobody picks one any other way.
    pub(super) fn preview_row(&self) {
        if !matches!(self.inner.source.get_untracked(), Some(Source::Themes)) {
            return;
        }
        let Some(Target::Theme(name)) = self.selected().map(|row| row.target) else {
            return;
        };
        // The theme the caret lands on is usually the one already in force, and applying a theme
        // that is already applied would rebuild every style for nothing.
        if self
            .inner
            .settings
            .with_untracked(|config| config.ui.theme == name)
        {
            return;
        }
        self.inner
            .settings
            .update(|config| config.ui.theme.clone_from(&name));
    }

    /// Takes what has been typed and searches or ranks again.
    pub fn set_query(&self, query: &str) {
        if self.inner.query.with_untracked(|held| held == query) {
            return;
        }
        self.inner.query.set(query.to_owned());
        let Some(source) = self.inner.source.get_untracked() else {
            return;
        };
        if source.is_live() {
            self.start_grep(&source, query);
        } else {
            self.rank(query);
        }
    }

    /// Does what the row the caret is on says, and closes.
    pub fn activate(&self) {
        let Some(row) = self.selected() else {
            return;
        };
        let workspace = self.inner.workspace.clone();
        // Whatever a preview showed is what was chosen, so there is nothing to put back.
        self.inner.restore.borrow_mut().take();
        self.close();

        match row.target {
            Target::File { path, line, .. } => crate::files::open_at(&workspace, path, line),
            Target::Buffer(id) => workspace.show(id),
            Target::Line(line) => {
                if let Some(buffer) = workspace.current_buffer() {
                    crate::files::go_to(&workspace, buffer.id, line);
                }
            }
            Target::Theme(name) => {
                self.inner
                    .settings
                    .update(|config| config.ui.theme = name.clone());
                workspace.say(name);
            }
            Target::Action(action) => {
                if let Some(vim) = zgui::reactive::use_local_context::<crate::vim::Vim>() {
                    vim.run(&action);
                }
            }
            Target::Run(deed) => deed.run(),
            Target::Nothing => {}
        }
    }
}
