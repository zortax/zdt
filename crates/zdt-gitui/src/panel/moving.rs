//! Opening the panel, closing it, and moving the caret in it.

use super::*;

impl GitUi {
    /// Opens the modal and reads everything.
    pub fn open(&self) {
        if !self.is_repository() {
            self.inner
                .host
                .say("this project is not in a git repository");
            return;
        }
        self.inner.open.set(true);
        self.focus();
        self.refresh();
        self.watch();
    }

    /// Puts the modal away, and gives the keyboard back to the editor.
    pub fn close(&self) {
        if self.inner.open.get_untracked() {
            self.inner.open.set(false);
        }
        if self.inner.message.get_untracked().is_some() {
            self.inner.message.set(None);
        }
        self.blur();
        self.inner.host.release_keyboard();
    }

    /// Opens it as a tab instead.
    pub fn open_tab(&self) {
        if !self.is_repository() {
            self.inner
                .host
                .say("this project is not in a git repository");
            return;
        }
        self.close();
        self.inner.host.open_as_tab();
        self.refresh();
        self.watch();
    }

    /// Shows one particular half, which is what `1` and `2` do.
    pub fn show(&self, wanted: View) {
        if self.inner.view.get_untracked() != wanted {
            self.toggle_view();
        }
    }

    /// Shows the other half.
    pub fn toggle_view(&self) {
        let next = match self.inner.view.get_untracked() {
            View::Status => View::History,
            View::History => View::Status,
        };
        self.inner.view.set(next);
        self.inner.list.set(match next {
            View::Status => List::Unstaged,
            View::History => List::History,
        });
        self.load_diff();
    }

    /// Moves the keys to the next list, wrapping.
    pub fn cycle_list(&self, forward: bool) {
        let order: &[List] = match self.inner.view.get_untracked() {
            View::Status => &[List::Unstaged, List::Staged, List::Diff, List::Branches],
            View::History => &[List::History, List::Diff, List::Branches],
        };
        let here = order
            .iter()
            .position(|one| *one == self.inner.list.get_untracked())
            .unwrap_or(0) as isize;
        let step = if forward { 1 } else { -1 };
        let next = order[(here + step).rem_euclid(order.len() as isize) as usize];
        self.inner.list.set(next);
        self.load_diff();
    }

    /// Puts the keys in one particular list.
    pub fn set_list(&self, wanted: List) {
        if self.inner.list.get_untracked() != wanted {
            self.inner.list.set(wanted);
            self.load_diff();
        }
    }

    /// Turns the diff between one column and two.
    pub fn toggle_side_by_side(&self) {
        let now = !self.inner.side_by_side.get_untracked();
        self.inner.side_by_side.set(now);
    }

    // ---- Moving ------------------------------------------------------------------------------

    /// Moves the caret in whichever list has the keys.
    pub fn step(&self, offset: isize) {
        let focus = self.inner.list.get_untracked();
        let (signal, count) = match focus {
            List::Branches => (
                self.inner.at_branch,
                self.inner.branches.with_untracked(Vec::len),
            ),
            List::Unstaged => (self.inner.at_unstaged, self.unstaged_untracked().len()),
            List::Staged => (self.inner.at_staged, self.staged_untracked().len()),
            List::History => (
                self.inner.at_history,
                self.inner.commits.with_untracked(Vec::len),
            ),
            List::Diff => (self.inner.at_diff, self.diff_row_count()),
        };
        if count == 0 {
            return;
        }
        // Clamped, and never wrapped. Somebody reads these lists in order, and a `j` at the
        // bottom that jumps to the top loses their place.
        let next = (signal.get_untracked() as isize + offset).clamp(0, count as isize - 1) as usize;
        if next != signal.get_untracked() {
            signal.set(next);
            if focus != List::Diff {
                self.load_diff();
            }
        }
    }

    /// Puts the caret on one particular row of one particular list.
    ///
    /// What a click does. The caller names the list, because a click can land in a list the keys
    /// were not in. Most clicks do.
    pub fn go_to(&self, which: List, index: usize) {
        let signal = match which {
            List::Branches => self.inner.at_branch,
            List::Unstaged => self.inner.at_unstaged,
            List::Staged => self.inner.at_staged,
            List::History => self.inner.at_history,
            List::Diff => self.inner.at_diff,
        };
        if signal.get_untracked() == index {
            return;
        }
        signal.set(index);
        // The diff is a view *of* the selection, so moving inside it changes nothing to read.
        if which != List::Diff {
            self.load_diff();
        }
    }

    /// To the top of the list with the keys.
    pub fn to_top(&self) {
        self.step(isize::MIN / 2);
    }

    /// To the bottom of it.
    pub fn to_bottom(&self) {
        self.step(isize::MAX / 2);
    }

    /// How many rows the diff is drawn as.
    pub(super) fn diff_row_count(&self) -> usize {
        self.inner
            .diff
            .with_untracked(|files| diff_rows(files).len())
    }

    /// The unstaged entries, without subscribing.
    pub(super) fn unstaged_untracked(&self) -> Vec<Entry> {
        self.inner.entries.with_untracked(|entries| {
            entries
                .iter()
                .filter(|e| e.is_unstaged())
                .cloned()
                .collect()
        })
    }

    /// The staged ones.
    pub(super) fn staged_untracked(&self) -> Vec<Entry> {
        self.inner
            .entries
            .with_untracked(|entries| entries.iter().filter(|e| e.is_staged()).cloned().collect())
    }
}
