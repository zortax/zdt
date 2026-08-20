//! The git panel's state.
//!
//! An `Rc` of signals, every piece of work on a worker, and a generation counter. An answer for a
//! question nobody is asking any more is dropped and not drawn.
//!
//! Reading a repository is slow enough to matter. A status walks the working tree and a log walks
//! the object store. The interface thread waits for neither.

mod changing;
mod frame;
mod load;
mod modal;
mod moving;
mod read;

pub use crate::panel::frame::{GitPanel, GitPanelProps};
pub use crate::panel::modal::{GitModal, GitModalProps};

/// How tall one row of any of the lists is.
///
/// Declared and not measured, because that is what a virtual list needs. The window is decided
/// before its rows are built, and a height taken from the rows would mean building all of them to
/// find out which to build.
pub(crate) const ROW: f32 = 22.0;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use zdt_git::{Branch, Commit, Entry, FileDiff, Repo, Row};
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

use crate::diff::{DiffRow, diff_rows};
use crate::host::Host;

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
/// The view does not decide this. The status side has three lists: the branches, the unstaged
/// files and the staged ones. Which one the keys move in is the whole of what `<Tab>` changes.
///
/// This differs from whether the panel has the *keyboard*, which is [`GitUi::is_focused`]. One
/// says which list a key moves in. The other says whether keys arrive at all.
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
    /// The application the panel is inside.
    ///
    /// Taken once at construction. Every one of this panel's operations reports from inside a
    /// task, and a context looked up after an await is gone.
    host: Rc<dyn Host>,
    /// The repository, when the project is in one.
    repo: RefCell<Option<Repo>>,
    /// Whether the modal is up. The tab is a buffer and is not this.
    open: RwSignal<bool, LocalStorage>,
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
    /// Rows, and not hunks, so `j` moves one line and a long hunk can be read. What `s` stages is
    /// the hunk the caret's row belongs to. See [`GitUi::current_hunk`].
    at_diff: RwSignal<usize, LocalStorage>,

    /// The diff of whatever is selected.
    diff: RwSignal<Vec<FileDiff>, LocalStorage>,
    /// Which file of a commit's diff is expanded, when one is.
    at_file: RwSignal<usize, LocalStorage>,
    /// Whether the diff is shown side by side. One column otherwise.
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
    /// The same, for the diff. Walking a list quickly must not draw a diff already left.
    diff_generation: Cell<u64>,
    /// What is watching `.git`, held so that dropping this stops the watching.
    watcher: RefCell<Option<zdt_view::Watcher>>,
}

impl GitUi {
    /// Nothing read yet.
    ///
    /// `root` is the directory the repository is looked for at. `host` is the application around
    /// the panel; [`Nowhere`](crate::Nowhere) is the one that is nothing at all.
    #[must_use]
    pub fn new(root: &Path, host: Rc<dyn Host>) -> Self {
        let repo = Repo::open(root).ok();
        Self {
            inner: Rc::new(Inner {
                host,
                repo: RefCell::new(repo),
                open: RwSignal::new_local(false),
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
}
