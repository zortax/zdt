//! The git panel's state.
//!
//! The same shape as the picker and the file tree: an `Rc` of signals, every piece of work on a
//! worker, and a generation counter so that an answer for a question nobody is asking any more is
//! dropped rather than drawn.
//!
//! Reading a repository is slow enough to matter — a status walks the working tree, a log walks
//! the object store — and the interface thread never waits for either.
//!
//! # What it is looking at
//!
//! One of two things, and the panel is really two panels sharing a frame:
//!
//!   * **Status**: what has changed, split into staged and unstaged, with the diff of whichever
//!     file is selected. This is the daily-driver view.
//!   * **History**: the commit graph, with the details and diff of whichever commit is selected.
//!
//! They share the layout because they are the same shape — a list on the left, a diff on the right
//! — and switching between them with one key is the whole navigation.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use zdt_git::{Branch, Commit, Entry, FileDiff, Repo, Row};
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

use crate::workspace::Workspace;

/// How many commits are read at a time.
///
/// A screenful and a good deal more, so that scrolling does not stop; not the whole history, which
/// on a large project is a hundred thousand objects nobody is going to look at.
const PAGE: usize = 500;

/// Which half of the panel is showing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum View {
    /// What has changed, and what is staged.
    #[default]
    Status,
    /// The commit graph.
    History,
}

/// Which list the caret is in.
///
/// Not derivable from the view: the status side has three lists — the branches, the unstaged files
/// and the staged ones — and which one the keys move in is the whole of what `<Tab>` changes.
///
/// Distinct from whether the panel has the *keyboard*, which is [`GitUi::is_focused`]: one is
/// which list a key moves in, the other is whether keys arrive at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum List {
    /// The branch list down the side.
    Branches,
    /// The files that are not staged.
    #[default]
    Unstaged,
    /// The files that are.
    Staged,
    /// The commit graph.
    History,
    /// The diff itself, which can be scrolled and whose hunks can be staged.
    Diff,
}

/// One row of what is being shown, whichever view that is.
#[derive(Clone, PartialEq, Debug)]
pub enum Selected {
    /// A file, and whether the staged or unstaged side of it.
    File {
        /// Which file.
        path: String,
        /// Whether what is showing is the staged half.
        staged: bool,
    },
    /// A commit.
    Commit(String),
    /// Nothing at all.
    Nothing,
}

/// The git panel.
#[derive(Clone)]
pub struct GitUi {
    inner: Rc<Inner>,
}

struct Inner {
    workspace: Workspace,
    /// Where anything that went wrong is announced.
    ///
    /// Taken once at construction: every one of this panel's operations reports from inside a
    /// task, and a context looked up after an await is not there — see `tests/context.rs`.
    notify: Option<crate::notify::Notify>,
    /// The repository, when the project is in one.
    repo: RefCell<Option<Repo>>,
    /// Whether the modal is up. The tab is a buffer and is not this.
    open: RwSignal<bool, LocalStorage>,
    /// Whether the panel has the keyboard.
    ///
    /// Held here rather than read from the element, because both presentations share it: the
    /// modal claims it when it opens, and the tab claims it when its pane is the focused one.
    focused: RwSignal<bool, LocalStorage>,
    /// Which half is showing.
    view: RwSignal<View, LocalStorage>,
    /// Which list the keys move in.
    list: RwSignal<List, LocalStorage>,

    /// What has changed.
    entries: RwSignal<Vec<Entry>, LocalStorage>,
    /// The commits, newest first.
    commits: RwSignal<Vec<Commit>, LocalStorage>,
    /// Where each of them goes in the drawing.
    rows: RwSignal<Vec<Row>, LocalStorage>,
    /// The branches.
    branches: RwSignal<Vec<Branch>, LocalStorage>,
    /// What `HEAD` says.
    head: RwSignal<String, LocalStorage>,

    /// Which row of each list the caret is on.
    at_unstaged: RwSignal<usize, LocalStorage>,
    at_staged: RwSignal<usize, LocalStorage>,
    at_history: RwSignal<usize, LocalStorage>,
    at_branch: RwSignal<usize, LocalStorage>,
    /// Which *row* of the diff the caret is on.
    ///
    /// Rows rather than hunks, so that `j` moves one line and a long hunk can be read. Which hunk
    /// `s` stages is the hunk the caret's row belongs to — see [`GitUi::current_hunk`].
    at_diff: RwSignal<usize, LocalStorage>,

    /// The diff of whatever is selected.
    diff: RwSignal<Vec<FileDiff>, LocalStorage>,
    /// Which file of a commit's diff is expanded, when one is.
    at_file: RwSignal<usize, LocalStorage>,
    /// Whether the diff is shown side by side rather than one column.
    side_by_side: RwSignal<bool, LocalStorage>,

    /// What is being typed as a commit message, when a commit is being written.
    message: RwSignal<Option<String>, LocalStorage>,
    /// Whether that commit would replace the last one.
    amending: Cell<bool>,

    /// Whether anything is being read.
    working: RwSignal<bool, LocalStorage>,
    /// What went wrong, when something did.
    problem: RwSignal<Option<String>, LocalStorage>,
    /// Which question is being answered.
    generation: Cell<u64>,
    /// The same, for the diff — walking a list quickly must not draw a diff already left.
    diff_generation: Cell<u64>,
    /// What is watching `.git`, held so that dropping this stops the watching.
    watcher: RefCell<Option<crate::reload::Watcher>>,
}

impl GitUi {
    /// Nothing read yet.
    #[must_use]
    pub fn new(workspace: Workspace) -> Self {
        let repo = Repo::open(workspace.project().root()).ok();
        Self {
            inner: Rc::new(Inner {
                workspace,
                notify: crate::notify::use_notify(),
                repo: RefCell::new(repo),
                open: RwSignal::new_local(false),
                focused: RwSignal::new_local(false),
                view: RwSignal::new_local(View::default()),
                list: RwSignal::new_local(List::default()),
                entries: RwSignal::new_local(Vec::new()),
                commits: RwSignal::new_local(Vec::new()),
                rows: RwSignal::new_local(Vec::new()),
                branches: RwSignal::new_local(Vec::new()),
                head: RwSignal::new_local(String::new()),
                at_unstaged: RwSignal::new_local(0),
                at_staged: RwSignal::new_local(0),
                at_history: RwSignal::new_local(0),
                at_branch: RwSignal::new_local(0),
                at_diff: RwSignal::new_local(0),
                diff: RwSignal::new_local(Vec::new()),
                at_file: RwSignal::new_local(0),
                side_by_side: RwSignal::new_local(false),
                message: RwSignal::new_local(None),
                amending: Cell::new(false),
                working: RwSignal::new_local(false),
                problem: RwSignal::new_local(None),
                generation: Cell::new(0),
                diff_generation: Cell::new(0),
                watcher: RefCell::new(None),
            }),
        }
    }

    // ---- What the interface reads ------------------------------------------------------------

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
    /// there are without building any of them. A commit that touches forty files is a few thousand
    /// rows, and building them all to show thirty is what made the panel unusable.
    #[must_use]
    pub fn diff_rows(&self) -> Vec<DiffRow> {
        diff_rows(&self.inner.diff.get())
    }

    /// Whether the panel has the keyboard. Tracked.
    #[must_use]
    pub fn is_focused(&self) -> bool {
        self.inner.focused.get()
    }

    /// The same, without subscribing.
    #[must_use]
    pub fn is_focused_untracked(&self) -> bool {
        self.inner.focused.get_untracked()
    }

    /// Says the panel has the keyboard.
    pub fn focus(&self) {
        if !self.inner.focused.get_untracked() {
            self.inner.focused.set(true);
        }
    }

    /// Says it does not.
    pub fn blur(&self) {
        if self.inner.focused.get_untracked() {
            self.inner.focused.set(false);
        }
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

    // ---- Opening and closing -----------------------------------------------------------------

    /// Opens the modal and reads everything.
    pub fn open(&self) {
        if !self.is_repository() {
            self.inner
                .workspace
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
        self.inner.workspace.focus_editor();
    }

    /// Opens it as a tab instead.
    pub fn open_tab(&self) {
        if !self.is_repository() {
            self.inner
                .workspace
                .say("this project is not in a git repository");
            return;
        }
        self.close();
        self.inner
            .workspace
            .open_panel(crate::workspace::BufferKind::Git);
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
        // Clamped rather than wrapped: these are lists somebody is reading in order, and a `j` at
        // the bottom that jumps to the top is a `j` that loses their place.
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
    /// What a click does. The list is named rather than assumed, because a click can land in a
    /// list the keys were not in — which is most clicks.
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
    fn diff_row_count(&self) -> usize {
        self.inner
            .diff
            .with_untracked(|files| diff_rows(files).len())
    }

    /// The unstaged entries, without subscribing.
    fn unstaged_untracked(&self) -> Vec<Entry> {
        self.inner.entries.with_untracked(|entries| {
            entries
                .iter()
                .filter(|e| e.is_unstaged())
                .cloned()
                .collect()
        })
    }

    /// The staged ones.
    fn staged_untracked(&self) -> Vec<Entry> {
        self.inner
            .entries
            .with_untracked(|entries| entries.iter().filter(|e| e.is_staged()).cloned().collect())
    }

    // ---- Reading -----------------------------------------------------------------------------

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
        crate::task::detached(async move {
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
    /// Called after a read, because a file that has just been staged is a file that has left one
    /// list and joined another — and a caret past the end of a list draws nothing.
    fn clamp(&self) {
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
    /// Not debounced: a diff is one file, and the wait somebody would notice is the one where
    /// walking a list leaves the panel beside it blank.
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
        crate::task::detached(async move {
            let read = zgui::task::blocking(move || match &what {
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
            })
            .await;

            // Walking a list quickly must not draw a diff the caret has already left.
            if git.inner.diff_generation.get() != generation {
                return;
            }
            git.inner.diff.set(read);
        });
    }

    /// Watches `.git` so that a commit made in a terminal shows up here.
    ///
    /// The directory rather than the files: git replaces `HEAD` and the index rather than writing
    /// them, so a watch on the file itself would follow the one that was renamed away — the same
    /// reason the configuration watcher watches its directory.
    fn watch(&self) {
        if self.inner.watcher.borrow().is_some() {
            return;
        }
        let Some(repo) = self.inner.repo.borrow().clone() else {
            return;
        };
        let git = self.clone();
        let paths = zdt_core::config::Paths::at(repo.dot_git());
        let watcher = crate::reload::watch(&paths, move || git.refresh());
        *self.inner.watcher.borrow_mut() = watcher;
    }

    // ---- Changing ----------------------------------------------------------------------------

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
    fn stage_selected(&self, staging: bool) {
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
    fn stage_hunk(&self) {
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
    fn unstage_hunk(&self) {
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
    /// The one thing here that loses work, so it is the one thing the panel asks about first —
    /// see the confirmation in [`crate::ui::git`].
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
            self.inner.workspace.say("nothing is staged");
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
    fn change(&self, work: impl FnOnce(&Repo) -> Result<(), zdt_git::Error> + Send + 'static) {
        let Some(repo) = self.inner.repo.borrow().clone() else {
            return;
        };
        let git = self.clone();
        crate::task::detached(async move {
            let done = zgui::task::blocking(move || work(&repo)).await;
            match done {
                Ok(()) => {
                    git.inner.problem.set(None);
                    git.refresh();
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
    fn change_with_git(
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
        crate::task::detached(async move {
            let done = zgui::task::blocking(move || work(&root)).await;
            match done {
                Ok(output) if output.status.success() => {
                    git.inner.problem.set(None);
                    git.refresh();
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
        crate::files::open(&self.inner.workspace, path);
    }

    /// Says what went wrong, wherever there is to say it.
    fn complain(&self, said: String) {
        match self.inner.notify.as_ref() {
            Some(notify) => notify.fail("git", Some(said)),
            None => self.inner.workspace.complain(said),
        }
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
        let rows = diff_rows(&files);

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

/// One row of the diff, as it is drawn.
///
/// The diff arrives as files holding hunks holding lines, which is the right shape to *think*
/// about and the wrong one to draw: a virtual list has to know how many rows there are without
/// building any of them, and a nested structure cannot say. So it is flattened once, here.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum DiffRow {
    /// A file's heading, and how much it changed.
    File {
        /// Which file.
        path: String,
        /// How many lines it adds.
        added: usize,
        /// How many it takes away.
        removed: usize,
        /// Whether there is nothing to show because it is not text.
        binary: bool,
    },
    /// A hunk's `@@` line.
    Hunk {
        /// What it says.
        header: String,
        /// Which hunk of the whole diff this is.
        hunk: usize,
    },
    /// One line of one hunk.
    Line {
        /// What happened to it.
        kind: zdt_git::LineKind,
        /// The text.
        text: String,
        /// Its number in the old file, when it has one.
        old: Option<u32>,
        /// Its number in the new file.
        new: Option<u32>,
        /// Which hunk it belongs to.
        hunk: usize,
    },
}

impl DiffRow {
    /// Which hunk this row belongs to, when it belongs to one.
    #[must_use]
    pub const fn hunk(&self) -> Option<usize> {
        match self {
            Self::File { .. } => None,
            Self::Hunk { hunk, .. } | Self::Line { hunk, .. } => Some(*hunk),
        }
    }
}

/// Every file's diff, as one flat list of rows.
#[must_use]
pub fn diff_rows(files: &[FileDiff]) -> Vec<DiffRow> {
    let mut rows = Vec::new();
    let mut hunk = 0;

    for file in files {
        let (added, removed) = file.counts();
        rows.push(DiffRow::File {
            path: file.path.clone(),
            added,
            removed,
            binary: file.binary,
        });
        for one in &file.hunks {
            rows.push(DiffRow::Hunk {
                header: one.header(),
                hunk,
            });
            rows.extend(one.lines.iter().map(|line| DiffRow::Line {
                kind: line.kind,
                text: line.text.clone(),
                old: line.old,
                new: line.new,
                hunk,
            }));
            hunk += 1;
        }
    }
    rows
}

/// Puts the panel where every component can find it.
pub fn provide(git: GitUi) {
    zgui::reactive::provide_local_context(git);
}

/// It, from inside a component.
///
/// # Panics
///
/// If none was provided above this component, which is a wiring mistake.
#[must_use]
pub fn use_gitui() -> GitUi {
    zgui::reactive::use_local_context::<GitUi>().expect("a git panel is provided at the root")
}

/// How long ago a unix timestamp was, roughly.
///
/// The same words the blame line uses, so that "3 days ago" means the same thing in both.
#[must_use]
pub fn ago(when: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(when);
    let seconds = (now - when).max(0);

    let (count, unit) = match seconds {
        ..60 => return "just now".to_owned(),
        60..3_600 => (seconds / 60, "minute"),
        3_600..86_400 => (seconds / 3_600, "hour"),
        86_400..2_592_000 => (seconds / 86_400, "day"),
        2_592_000..31_536_000 => (seconds / 2_592_000, "month"),
        _ => (seconds / 31_536_000, "year"),
    };
    let plural = if count == 1 { "" } else { "s" };
    format!("{count} {unit}{plural} ago")
}

/// The same, short enough for a column: `3d`, `2mo`.
#[must_use]
pub fn ago_short(when: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(when);
    let seconds = (now - when).max(0);

    match seconds {
        ..60 => "now".to_owned(),
        60..3_600 => format!("{}m", seconds / 60),
        3_600..86_400 => format!("{}h", seconds / 3_600),
        86_400..2_592_000 => format!("{}d", seconds / 86_400),
        2_592_000..31_536_000 => format!("{}mo", seconds / 2_592_000),
        _ => format!("{}y", seconds / 31_536_000),
    }
}

/// Everything a path could be, as one glyph and a tone.
#[must_use]
pub fn state_mark(state: zdt_git::State) -> (&'static str, &'static str) {
    use zdt_git::State;

    match state {
        State::Untracked => ("?", "zdt-git-untracked"),
        State::Added => ("A", "zdt-git-added"),
        State::Modified => ("M", "zdt-git-changed"),
        State::Deleted => ("D", "zdt-git-removed"),
        State::Renamed => ("R", "zdt-git-changed"),
        State::Conflicted => ("U", "zdt-git-conflict"),
        State::Unchanged => (" ", "zui-color-muted-foreground"),
    }
}

#[cfg(test)]
mod tests {
    use super::{ago, ago_short};

    /// A timestamp `seconds` ago.
    fn back(seconds: i64) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is after 1970")
            .as_secs() as i64
            - seconds
    }

    #[test]
    fn a_moment_ago_is_just_now() {
        assert_eq!(ago(back(5)), "just now");
        assert_eq!(ago_short(back(5)), "now");
    }

    #[test]
    fn one_of_something_is_singular() {
        // "1 days ago" is the kind of thing that makes an interface feel unfinished.
        assert_eq!(ago(back(60 * 60 * 24)), "1 day ago");
        assert_eq!(ago(back(60 * 60 * 24 * 2)), "2 days ago");
        assert_eq!(ago(back(60 * 60)), "1 hour ago");
    }

    #[test]
    fn the_short_form_fits_a_column() {
        for seconds in [30, 300, 7_200, 86_400 * 3, 86_400 * 60, 86_400 * 800] {
            let short = ago_short(back(seconds));
            assert!(short.len() <= 4, "{short} is too wide for the column");
        }
    }

    #[test]
    fn a_timestamp_in_the_future_is_not_negative() {
        // Which happens with a clock that has been put back, and "-4 hours ago" is worse than
        // being a moment out.
        assert_eq!(ago(back(-500)), "just now");
    }
}
