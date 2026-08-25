//! Reading the repository.

use super::*;

impl GitUi {
    /// Reads everything again: the status, the history, the branches.
    ///
    /// All of it on one worker, because all three questions want the same repository and opening
    /// it three times would be three times the work for one answer.
    pub fn refresh(&self) {
        let Some(repo) = self.inner.repo.borrow().clone() else {
            return;
        };
        let generation = self.inner.generation.get() + 1;
        self.inner.generation.set(generation);
        self.inner.working.set(true);

        let git = self.clone();
        zdt_view::detached(async move {
            let read = zgui::task::blocking(move || {
                let entries = zdt_git::status::status(&repo).unwrap_or_default();
                let commits = zdt_git::log::log(&repo, None, PAGE).unwrap_or_default();
                let rows = zdt_git::graph::lay_out(&commits);
                let branches = zdt_git::branches(&repo).unwrap_or_default();
                let head = zdt_git::head(&repo)
                    .map(|head| head.label())
                    .unwrap_or_default();
                (entries, commits, rows, branches, head)
            })
            .await;

            // An answer for a question nobody is asking any more.
            if git.inner.generation.get() != generation {
                return;
            }
            let (entries, commits, rows, branches, head) = read;
            git.inner.entries.set(entries);
            git.inner.commits.set(commits);
            git.inner.rows.set(rows);
            git.inner.branches.set(branches);
            git.inner.head.set(head);
            git.inner.working.set(false);
            git.clamp();
            git.load_diff();
        });
    }

    /// Keeps every caret inside the list it is in.
    ///
    /// Called after a read. A file that has just been staged has left one list and joined
    /// another, and a caret past the end of a list draws nothing.
    pub(super) fn clamp(&self) {
        let pairs = [
            (self.inner.at_unstaged, self.unstaged_untracked().len()),
            (self.inner.at_staged, self.staged_untracked().len()),
            (
                self.inner.at_history,
                self.inner.commits.with_untracked(Vec::len),
            ),
            (
                self.inner.at_branch,
                self.inner.branches.with_untracked(Vec::len),
            ),
        ];
        for (signal, count) in pairs {
            let most = count.saturating_sub(1);
            if signal.get_untracked() > most {
                signal.set(most);
            }
        }
    }

    /// Reads the diff of whatever is selected, after nothing at all.
    ///
    /// It is not debounced. A diff is one file, and the wait somebody would notice is the one
    /// where walking a list leaves the panel beside it blank.
    pub fn load_diff(&self) {
        let Some(repo) = self.inner.repo.borrow().clone() else {
            return;
        };
        let generation = self.inner.diff_generation.get() + 1;
        self.inner.diff_generation.set(generation);
        self.inner.at_diff.set(0);
        self.inner.at_file.set(0);

        let what = self.selected();
        let git = self.clone();
        zdt_view::detached(async move {
            let read = zgui::task::blocking(move || {
                let files = match &what {
                    Selected::File { path, staged } => {
                        let found = if *staged {
                            zdt_git::diff::staged(&repo, path)
                        } else {
                            zdt_git::diff::worktree(&repo, path)
                        };
                        vec![found.unwrap_or_else(|_| FileDiff::empty(path.clone()))]
                    }
                    Selected::Commit(id) => zdt_git::diff::commit(&repo, id).unwrap_or_default(),
                    Selected::Nothing => Vec::new(),
                };
                // Coloured here, on the worker: a parse is too slow for the interface thread,
                // and the cache makes asking again about an unchanged file free.
                let marks = files.iter().map(zdt_syntax::marks_of).collect::<Vec<_>>();
                (files, marks)
            })
            .await;

            // Walking a list quickly must not draw a diff the caret has already left.
            if git.inner.diff_generation.get() != generation {
                return;
            }
            let (files, marks) = read;
            git.inner.flat.set(Rc::new(diff_rows(&files)));
            git.inner.marks.set(Rc::new(marks));
            git.inner.diff.set(files);
        });
    }

    /// Watches `.git` so that a commit made in a terminal shows up here.
    ///
    /// The directory, and not the files. Git replaces `HEAD` and the index instead of writing
    /// into them, so a watch on the file itself would follow the one that was renamed away. The
    /// configuration watcher watches its directory for the same reason.
    pub(super) fn watch(&self) {
        if self.inner.watcher.borrow().is_some() {
            return;
        }
        let Some(repo) = self.inner.repo.borrow().clone() else {
            return;
        };
        let git = self.clone();
        let directory = repo.dot_git().to_path_buf();
        let watcher = zdt_view::watch(&directory, move || git.refresh());
        *self.inner.watcher.borrow_mut() = watcher;
    }
}
