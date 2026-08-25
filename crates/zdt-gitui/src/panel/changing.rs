//! Staging, committing, and everything else that writes.

use super::*;

impl GitUi {
    /// Stages whatever the caret is on: a whole file, or one hunk of it.
    pub fn stage(&self) {
        match self.inner.list.get_untracked() {
            List::Diff => self.stage_hunk(),
            _ => self.stage_selected(true),
        }
    }

    /// Unstages it.
    pub fn unstage(&self) {
        match self.inner.list.get_untracked() {
            List::Diff => self.unstage_hunk(),
            _ => self.stage_selected(false),
        }
    }

    /// Stages or unstages the whole file the caret is on.
    pub(super) fn stage_selected(&self, staging: bool) {
        let Selected::File { path, .. } = self.selected() else {
            return;
        };
        self.change(move |repo| {
            if staging {
                zdt_git::stage::stage_file(repo, &path)
            } else {
                zdt_git::stage::unstage_file(repo, &path)
            }
        });
    }

    /// Stages the one hunk the caret is on.
    pub(super) fn stage_hunk(&self) {
        let Some((path, hunk)) = self.current_hunk() else {
            return;
        };
        // Which way round depends on which side of the panel the diff came from: the staged list
        // shows what is in the index, and "stage" there means nothing.
        let staged = matches!(self.selected(), Selected::File { staged: true, .. });
        self.change(move |repo| {
            if staged {
                Ok(())
            } else {
                zdt_git::stage::stage_hunks(repo, &path, std::slice::from_ref(&hunk))
            }
        });
    }

    /// Takes it back out.
    pub(super) fn unstage_hunk(&self) {
        let Some((path, hunk)) = self.current_hunk() else {
            return;
        };
        let staged = matches!(self.selected(), Selected::File { staged: true, .. });
        self.change(move |repo| {
            if staged {
                zdt_git::stage::unstage_hunks(repo, &path, std::slice::from_ref(&hunk))
            } else {
                Ok(())
            }
        });
    }

    /// Throws away whatever the caret is on.
    ///
    /// The one thing here that loses work, so it is the one thing the panel asks about first.
    /// See the confirmation in [`crate::panel::frame`].
    pub fn discard(&self) {
        let Selected::File { path, staged } = self.selected() else {
            return;
        };
        if staged {
            // Discarding something staged means unstaging it; throwing away a staged change
            // outright is two operations and should be asked for as two.
            self.stage_selected(false);
            return;
        }
        let focus = self.inner.list.get_untracked();
        let hunk = (focus == List::Diff).then(|| self.current_hunk()).flatten();
        self.change(move |repo| match &hunk {
            Some((path, hunk)) => {
                zdt_git::stage::discard_hunks(repo, path, std::slice::from_ref(hunk))
            }
            None => zdt_git::stage::discard_file(repo, &path),
        });
    }

    /// Stages everything.
    pub fn stage_all(&self) {
        let paths: Vec<String> = self
            .unstaged_untracked()
            .into_iter()
            .map(|entry| entry.path)
            .collect();
        self.change(move |repo| {
            for path in &paths {
                zdt_git::stage::stage_file(repo, path)?;
            }
            Ok(())
        });
    }

    /// Unstages everything.
    pub fn unstage_all(&self) {
        let paths: Vec<String> = self
            .staged_untracked()
            .into_iter()
            .map(|entry| entry.path)
            .collect();
        self.change(move |repo| {
            for path in &paths {
                zdt_git::stage::unstage_file(repo, path)?;
            }
            Ok(())
        });
    }

    /// Opens the box a commit message is typed into.
    pub fn start_commit(&self, amend: bool) {
        if !amend && self.staged_untracked().is_empty() {
            self.inner.host.say("nothing is staged");
            return;
        }
        self.inner.amending.set(amend);
        // Amending opens holding the message being replaced, which is the whole point of amending
        // for most of the times anybody does it.
        let start = if amend {
            self.inner
                .commits
                .with_untracked(|commits| commits.first().map(|one| one.summary.clone()))
                .unwrap_or_default()
        } else {
            String::new()
        };
        self.inner.message.set(Some(start));
    }

    /// Puts that box away without committing.
    pub fn cancel_commit(&self) {
        self.inner.message.set(None);
    }

    /// Commits what is staged, with the message that was typed.
    pub fn commit(&self, message: &str) {
        let amend = self.inner.amending.get();
        let message = message.to_owned();
        self.inner.message.set(None);
        self.change(move |repo| zdt_git::commit(repo, &message, amend).map(|_| ()));
    }

    /// Checks out the branch the caret is on.
    pub fn checkout(&self) {
        if self.inner.list.get_untracked() != List::Branches {
            return;
        }
        let Some(branch) = self
            .inner
            .branches
            .with_untracked(|branches| branches.get(self.inner.at_branch.get_untracked()).cloned())
        else {
            return;
        };
        if branch.current {
            return;
        }
        // Through git itself: a checkout writes the working tree, updates the index and moves
        // `HEAD`, and doing two of those three correctly is worse than doing none of them.
        let name = branch.name.clone();
        self.change_with_git(move |root| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(root)
                .args(["checkout", &name])
                .output()
        });
    }

    /// Runs `work` against the repository, then reads everything again.
    pub(super) fn change(
        &self,
        work: impl FnOnce(&Repo) -> Result<(), zdt_git::Error> + Send + 'static,
    ) {
        let Some(repo) = self.inner.repo.borrow().clone() else {
            return;
        };
        let git = self.clone();
        zdt_view::detached(async move {
            let done = zgui::task::blocking(move || work(&repo)).await;
            match done {
                Ok(()) => {
                    git.inner.problem.set(None);
                    git.refresh();
                    git.inner.host.changed();
                }
                Err(error) => {
                    let said = error.to_string();
                    git.inner.problem.set(Some(said.clone()));
                    git.complain(said);
                }
            }
        });
    }

    /// The same, for the one operation that runs git itself.
    pub(super) fn change_with_git(
        &self,
        work: impl FnOnce(&std::path::Path) -> std::io::Result<std::process::Output> + Send + 'static,
    ) {
        let Some(root) = self
            .inner
            .repo
            .borrow()
            .as_ref()
            .map(|repo| repo.root().to_path_buf())
        else {
            return;
        };
        let git = self.clone();
        zdt_view::detached(async move {
            let done = zgui::task::blocking(move || work(&root)).await;
            match done {
                Ok(output) if output.status.success() => {
                    git.inner.problem.set(None);
                    git.refresh();
                    git.inner.host.changed();
                }
                Ok(output) => {
                    let said = String::from_utf8_lossy(&output.stderr);
                    let first = said
                        .lines()
                        .map(str::trim)
                        .find(|line| !line.is_empty())
                        .unwrap_or("git refused")
                        .to_owned();
                    git.inner.problem.set(Some(first.clone()));
                    git.complain(first);
                }
                Err(error) => git.complain(error.to_string()),
            }
        });
    }

    /// Opens whatever the caret is on in an editor, and closes the modal.
    pub fn open_selected(&self) {
        let path: Option<PathBuf> = match self.selected() {
            Selected::File { path, .. } => self
                .inner
                .repo
                .borrow()
                .as_ref()
                .map(|repo| repo.absolute(&path)),
            _ => None,
        };
        let Some(path) = path.filter(|path| path.exists()) else {
            return;
        };
        self.close();
        self.inner.host.open(&path);
    }

    /// Says what went wrong, wherever there is to say it.
    pub(super) fn complain(&self, said: String) {
        self.inner.host.complain(&said);
    }

    /// The hunk the caret's row belongs to, and which file it is in.
    ///
    /// The caret walks rows so that a long hunk can be read a line at a time; what `s` stages is
    /// the hunk that row is part of. A caret resting on a file's heading takes that file's first
    /// hunk, which is what somebody pressing `s` on a filename means.
    #[must_use]
    pub fn current_hunk(&self) -> Option<(String, zdt_git::DiffHunk)> {
        let at = self.inner.at_diff.get_untracked();
        let files = self.inner.diff.get_untracked();
        let rows = self.inner.flat.get_untracked();

        // From the caret's row forwards: a heading has no hunk of its own, and the hunk it means
        // is the next one under it.
        let found = rows.iter().skip(at).find_map(DiffRow::hunk)?;
        let mut passed = 0;
        for file in files {
            for hunk in &file.hunks {
                if passed == found {
                    return Some((file.path.clone(), hunk.clone()));
                }
                passed += 1;
            }
        }
        None
    }
}
