//! What the interface reads.

use super::*;

impl GitUi {
    /// The application the panel is inside.
    #[must_use]
    pub fn host(&self) -> Rc<dyn Host> {
        Rc::clone(&self.inner.host)
    }

    /// Whether the project is in a repository at all.
    #[must_use]
    pub fn is_repository(&self) -> bool {
        self.inner.repo.borrow().is_some()
    }

    /// Whether the modal is up. Tracked.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.inner.open.get()
    }

    /// Whether it is, without subscribing.
    #[must_use]
    pub fn is_open_untracked(&self) -> bool {
        self.inner.open.get_untracked()
    }

    /// Which half is showing. Tracked.
    #[must_use]
    pub fn view(&self) -> View {
        self.inner.view.get()
    }

    /// Which list the caret is in. Tracked.
    #[must_use]
    pub fn list(&self) -> List {
        self.inner.list.get()
    }

    /// What has changed and is not staged. Tracked.
    #[must_use]
    pub fn unstaged(&self) -> Vec<Entry> {
        self.inner
            .entries
            .get()
            .into_iter()
            .filter(Entry::is_unstaged)
            .collect()
    }

    /// What is staged. Tracked.
    #[must_use]
    pub fn staged(&self) -> Vec<Entry> {
        self.inner
            .entries
            .get()
            .into_iter()
            .filter(Entry::is_staged)
            .collect()
    }

    /// The commits. Tracked.
    #[must_use]
    pub fn commits(&self) -> Vec<Commit> {
        self.inner.commits.get()
    }

    /// Where each of them goes in the drawing. Tracked.
    #[must_use]
    pub fn rows(&self) -> Vec<Row> {
        self.inner.rows.get()
    }

    /// The branches. Tracked.
    #[must_use]
    pub fn branches(&self) -> Vec<Branch> {
        self.inner.branches.get()
    }

    /// What `HEAD` says. Tracked.
    #[must_use]
    pub fn head(&self) -> String {
        self.inner.head.get()
    }

    /// Which row of the list `which` the caret is on. Tracked.
    #[must_use]
    pub fn at(&self, which: List) -> usize {
        match which {
            List::Branches => self.inner.at_branch.get(),
            List::Unstaged => self.inner.at_unstaged.get(),
            List::Staged => self.inner.at_staged.get(),
            List::History => self.inner.at_history.get(),
            List::Diff => self.inner.at_diff.get(),
        }
    }

    /// The diff of whatever is selected. Tracked.
    #[must_use]
    pub fn diff(&self) -> Vec<FileDiff> {
        self.inner.diff.get()
    }

    /// The diff as one flat list of rows. Tracked.
    ///
    /// Flat because it is drawn by a virtual list, and a virtual list needs to know how many rows
    /// there are without building any of them. Flattened once beside the read, so asking is a
    /// reference count and never a walk.
    #[must_use]
    pub fn diff_rows(&self) -> Rc<Vec<DiffRow>> {
        self.inner.flat.get()
    }

    /// The syntax colours of each file of the diff, in the diff's order. Tracked.
    #[must_use]
    pub fn diff_marks(&self) -> Rc<Vec<zdt_syntax::DiffMarks>> {
        self.inner.marks.get()
    }

    /// Whether the panel has the keyboard. Tracked.
    ///
    /// The host's answer. Where the keyboard is belongs to the application around the panel, so
    /// the panel asks rather than remembering.
    #[must_use]
    pub fn is_focused(&self) -> bool {
        self.inner.host.has_keyboard()
    }

    /// Whether the diff is shown side by side. Tracked.
    #[must_use]
    pub fn is_side_by_side(&self) -> bool {
        self.inner.side_by_side.get()
    }

    /// The commit being written, when one is. Tracked.
    #[must_use]
    pub fn message(&self) -> Option<String> {
        self.inner.message.get()
    }

    /// Whether anything is being read. Tracked.
    #[must_use]
    pub fn is_working(&self) -> bool {
        self.inner.working.get()
    }

    /// What went wrong, when something did. Tracked.
    #[must_use]
    pub fn problem(&self) -> Option<String> {
        self.inner.problem.get()
    }

    /// The commit the caret is on, when the history is showing. Tracked.
    #[must_use]
    pub fn current_commit(&self) -> Option<Commit> {
        self.inner
            .commits
            .get()
            .get(self.inner.at_history.get())
            .cloned()
    }

    /// What the caret is on. Tracked.
    #[must_use]
    pub fn selected(&self) -> Selected {
        match self.inner.view.get() {
            View::History => match self.current_commit() {
                Some(commit) => Selected::Commit(commit.id),
                None => Selected::Nothing,
            },
            View::Status => {
                let staged = self.inner.list.get() == List::Staged;
                let list = if staged {
                    self.staged()
                } else {
                    self.unstaged()
                };
                let at = if staged {
                    self.inner.at_staged.get()
                } else {
                    self.inner.at_unstaged.get()
                };
                match list.get(at) {
                    Some(entry) => Selected::File {
                        path: entry.path.clone(),
                        staged,
                    },
                    None => Selected::Nothing,
                }
            }
        }
    }
}
