//! The file tree's state, as the interface reads it.
//!
//! The tree itself is [`zdt_core::tree`] and knows nothing about signals. This is what makes it
//! reactive, plus the two things a tree has that a list of rows does not: which row the caret is
//! on, and what is waiting to be pasted.
//!
//! Reading a directory is blocking, so every operation that reads one goes through a worker and
//! writes the answer back on the interface thread.

pub mod drag;
pub mod field;
pub mod leap;
pub mod menu;
pub mod tree;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use zdt_core::tree::{Filter, Row, Tree};
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

/// What a cut or a copy left waiting.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Clipboard {
    /// What is waiting.
    pub path: PathBuf,
    /// Whether pasting moves it. It copies otherwise.
    pub cut: bool,
}

/// The file tree.
#[derive(Clone)]
pub struct Explorer {
    inner: Rc<Inner>,
}

struct Inner {
    /// Where the keyboard is, for the whole session.
    ///
    /// Held rather than looked up, because `is_focused` is asked from timers and from tasks, and a
    /// context asked for there is a context that answers nothing.
    focus: crate::focus::Focusing,
    tree: RefCell<Tree>,
    /// The rows, as the list draws them.
    rows: RwSignal<Vec<Row>, LocalStorage>,
    /// Which row the caret is on.
    at: RwSignal<usize, LocalStorage>,
    /// Whether the panel is shown at all.
    ///
    /// Whether the keyboard is *in* it is [`crate::focus`]'s answer and never a flag here. Two
    /// places holding that fact is how a tree comes to believe it has a keyboard that is somewhere
    /// else.
    open: RwSignal<bool, LocalStorage>,
    /// What a cut or a copy left waiting.
    clipboard: RwSignal<Option<Clipboard>, LocalStorage>,
    /// Every row a person has picked out, beside the one the caret is on.
    ///
    /// By path, and never by index. A directory opening above them moves every index below it,
    /// and a selection that slid down a screen is one nobody meant.
    marked: RwSignal<Vec<PathBuf>, LocalStorage>,
    /// The pointer gesture that moves files from one directory to another.
    drag: crate::explorer::drag::Drag,
    /// Where the rows are drawn, once a panel has drawn them.
    ///
    /// No signal. Nothing on screen is decided by whether there is one, and an action that asks
    /// how far a half page is needs the answer now rather than on the next flush.
    viewport: RefCell<Option<crate::explorer::tree::Viewport>>,
    /// Whether the disk moved while the panel was closed.
    ///
    /// Reading a directory that nobody is looking at is work for nothing, so a change to a closed
    /// panel is remembered instead and read when it opens.
    stale: std::cell::Cell<bool>,
}

impl Explorer {
    /// A tree over `root`, closed.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>, filter: Filter, focus: crate::focus::Focusing) -> Self {
        Self {
            inner: Rc::new(Inner {
                focus,
                tree: RefCell::new(Tree::new(root, filter)),
                rows: RwSignal::new_local(Vec::new()),
                at: RwSignal::new_local(0),
                open: RwSignal::new_local(false),
                clipboard: RwSignal::new_local(None),
                marked: RwSignal::new_local(Vec::new()),
                drag: crate::explorer::drag::Drag::new(),
                viewport: RefCell::new(None),
                stale: std::cell::Cell::new(false),
            }),
        }
    }

    /// The rows. Tracked.
    #[must_use]
    pub fn rows(&self) -> Vec<Row> {
        self.inner.rows.get()
    }

    /// How many rows there are. Tracked, and narrower than reading them all.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.rows.with(Vec::len)
    }

    /// Whether there is nothing in it.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The row the caret is on. Tracked.
    #[must_use]
    pub fn at(&self) -> usize {
        self.inner.at.get()
    }

    /// The row the caret is on, without subscribing.
    #[must_use]
    pub fn at_untracked(&self) -> usize {
        self.inner.at.get_untracked()
    }

    /// Says where the rows are drawn.
    ///
    /// Registered when a panel is built and never taken back. A handle whose element has gone
    /// measures nothing and scrolls nothing, which is the right answer for a panel that is no
    /// longer there.
    pub fn set_viewport(&self, viewport: crate::explorer::tree::Viewport) {
        *self.inner.viewport.borrow_mut() = Some(viewport);
    }

    /// Where the rows are drawn, when a panel has drawn them.
    #[must_use]
    pub fn viewport(&self) -> Option<crate::explorer::tree::Viewport> {
        *self.inner.viewport.borrow()
    }

    /// How far a half page is, in rows.
    ///
    /// Ten before anything has been drawn, which is about a screenful of a small panel and is only
    /// ever used for a key pressed in a tree nobody can see yet.
    #[must_use]
    pub fn half_page(&self) -> usize {
        self.viewport()
            .map_or(10, crate::explorer::tree::Viewport::half_page)
    }

    /// Whether the panel is shown. Tracked.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.inner.open.get()
    }

    /// Whether the keyboard is in it. Tracked.
    #[must_use]
    pub fn is_focused(&self) -> bool {
        self.inner.focus.in_tree()
    }

    /// Whether the keyboard is in it, without subscribing.
    #[must_use]
    pub fn is_focused_untracked(&self) -> bool {
        self.inner.focus.in_tree_untracked()
    }

    /// What is waiting to be pasted. Tracked.
    #[must_use]
    pub fn clipboard(&self) -> Option<Clipboard> {
        self.inner.clipboard.get()
    }

    /// The row the caret is on, when there is one.
    #[must_use]
    pub fn selected(&self) -> Option<Row> {
        self.inner
            .rows
            .with_untracked(|rows| rows.get(self.inner.at.get_untracked()).cloned())
    }

    /// The directory a new file would go in: the selected directory, or the one holding the
    /// selected file, or the root.
    #[must_use]
    pub fn target_directory(&self) -> PathBuf {
        match self.selected() {
            Some(row) if row.entry.directory => row.entry.path,
            Some(row) => row
                .entry
                .path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.root()),
            None => self.root(),
        }
    }

    /// The directory the tree is rooted at.
    #[must_use]
    pub fn root(&self) -> PathBuf {
        self.inner.tree.borrow().root().to_path_buf()
    }

    // ---- Picking several out ------------------------------------------------------------

    /// Every row a person has picked out. Tracked.
    #[must_use]
    pub fn marked(&self) -> Vec<PathBuf> {
        self.inner.marked.get()
    }

    /// Whether `path` is one of them. Tracked.
    #[must_use]
    pub fn is_marked(&self, path: &Path) -> bool {
        self.inner
            .marked
            .with(|marked| marked.iter().any(|held| held == path))
    }

    /// Adds `at` to the set, or takes it out. What a control-click does.
    pub fn toggle_mark(&self, at: usize) {
        let Some(row) = self.row_at(at) else {
            return;
        };
        self.inner.marked.update(|marked| {
            match marked.iter().position(|held| *held == row.entry.path) {
                Some(index) => {
                    marked.remove(index);
                }
                None => marked.push(row.entry.path.clone()),
            }
        });
        self.go_to(at);
    }

    /// Picks out everything between the caret and `at`. What a shift-click does.
    pub fn mark_through(&self, at: usize) {
        let from = self.inner.at.get_untracked();
        let (first, last) = if from <= at { (from, at) } else { (at, from) };
        let paths: Vec<PathBuf> = self.inner.rows.with_untracked(|rows| {
            rows.get(first..=last.min(rows.len().saturating_sub(1)))
                .unwrap_or_default()
                .iter()
                .map(|row| row.entry.path.clone())
                .collect()
        });
        self.inner.marked.set(paths);
        self.go_to(at);
    }

    /// Forgets the set, which every ordinary click does.
    pub fn clear_marks(&self) {
        if !self.inner.marked.with_untracked(Vec::is_empty) {
            self.inner.marked.set(Vec::new());
        }
    }

    /// What an action should act on: everything picked out, or the row the caret is on.
    #[must_use]
    pub fn acting_on(&self) -> Vec<PathBuf> {
        let marked = self.inner.marked.get_untracked();
        if marked.is_empty() {
            self.selected()
                .map(|row| vec![row.entry.path])
                .unwrap_or_default()
        } else {
            marked
        }
    }

    /// The row at `at`, without subscribing.
    #[must_use]
    pub fn row_at(&self, at: usize) -> Option<Row> {
        self.inner.rows.with_untracked(|rows| rows.get(at).cloned())
    }

    // ---- Dragging -------------------------------------------------------------------------

    /// The pointer gesture that moves files.
    #[must_use]
    pub fn drag(&self) -> crate::explorer::drag::Drag {
        self.inner.drag
    }

    /// What a press on the row at `at` would carry.
    ///
    /// [`acting_on`](Self::acting_on)'s rule, read for a pointer: a press on a row that is one of
    /// the set takes the whole set, and a press on any other row clears the set and takes that row
    /// alone. Dragging one file out of three that are picked out would otherwise leave two of them
    /// lit up and untouched.
    pub fn carried_from(&self, at: usize) -> Vec<PathBuf> {
        let Some(row) = self.row_at(at) else {
            return Vec::new();
        };
        if self
            .inner
            .marked
            .with_untracked(|marked| marked.contains(&row.entry.path))
        {
            return self.inner.marked.get_untracked();
        }
        self.clear_marks();
        vec![row.entry.path]
    }

    /// Where `path` is in the list, when it is in it.
    #[must_use]
    pub fn index_of(&self, path: &Path) -> Option<usize> {
        self.inner.tree.borrow().index_of(path)
    }

    /// Whether `path` is a directory the tree has already read.
    #[must_use]
    pub fn is_directory(&self, path: &Path) -> bool {
        self.inner.tree.borrow().is_directory(path)
    }

    // ---- Moving about --------------------------------------------------------------------

    /// Moves the caret by `offset` rows, stopping at the ends.
    pub fn move_by(&self, offset: isize) {
        let count = self.inner.rows.with_untracked(Vec::len);
        if count == 0 {
            return;
        }
        let at = self.inner.at.get_untracked() as isize + offset;
        self.inner.at.set(at.clamp(0, count as isize - 1) as usize);
    }

    /// Puts the caret on `at`.
    pub fn go_to(&self, at: usize) {
        let count = self.inner.rows.with_untracked(Vec::len);
        if count > 0 {
            self.inner.at.set(at.min(count - 1));
        }
    }

    /// Puts the caret on the row for `path`, when it has one.
    pub fn go_to_path(&self, path: &Path) {
        if let Some(at) = self.inner.tree.borrow().index_of(path) {
            self.inner.at.set(at);
        }
    }

    // ---- Opening and closing -------------------------------------------------------------

    /// Shows or hides the panel, and moves the keyboard with it.
    ///
    /// What a key asking for the panel does: somebody who opened it means to use it.
    pub fn toggle(&self) {
        let open = !self.inner.open.get_untracked();
        self.set_open(open);
        if open {
            self.focus();
        } else {
            self.unfocus();
        }
    }

    /// Shows or hides the panel, leaving the keyboard where it is.
    ///
    /// What restoring a session does. Whether the panel was open and where the keyboard was are
    /// two facts, and putting the first one back must not answer the second.
    ///
    /// Opening it for the first time reads the root, which is why it takes a worker.
    pub fn set_open(&self, open: bool) {
        if self.inner.open.get_untracked() == open {
            return;
        }
        self.inner.open.set(open);
        if !open {
            return;
        }
        if self.inner.rows.with_untracked(Vec::is_empty) {
            self.expand_root();
        } else if self.inner.stale.get() {
            // The disk moved while nobody was looking.
            self.refresh();
        }
    }

    /// Hides the panel.
    pub fn close(&self) {
        self.inner.open.set(false);
        self.unfocus();
    }

    /// Gives the keyboard to the panel, opening it first when it is closed.
    pub fn focus(&self) {
        if !self.inner.open.get_untracked() {
            self.toggle();
            return;
        }
        self.inner.focus.enter_tree();
    }

    /// Takes the keyboard away, leaving the panel where it is.
    pub fn unfocus(&self) {
        self.inner.focus.enter_panes();
    }

    // ---- The tree itself -------------------------------------------------------------------

    /// Reads the root and shows it.
    pub fn expand_root(&self) {
        let root = self.root();
        self.with_tree_on_a_worker(move |tree| tree.expand(&root));
    }

    /// Opens the selected file, or steps into the selected directory. What `l` does.
    ///
    /// Answers the file to open, when the selected row is one. The caller opens it, because the
    /// tree has no business knowing what a buffer is.
    pub fn open_selected(&self) -> Option<PathBuf> {
        self.activate(false)
    }

    /// The same, except that a directory already open is *closed*. What `<CR>` and a click do.
    ///
    /// `l` and `<CR>` differ on purpose, as they do in neo-tree. `l` is a movement and steps into
    /// what is already open. `<CR>` and a click are the one gesture people expect to work both
    /// ways.
    pub fn toggle_selected(&self) -> Option<PathBuf> {
        self.activate(true)
    }

    /// Opens a file, or opens, steps into, or closes a directory.
    fn activate(&self, collapse: bool) -> Option<PathBuf> {
        let row = self.selected()?;
        if !row.entry.directory {
            return Some(row.entry.path);
        }
        let path = row.entry.path.clone();
        if self.inner.tree.borrow().is_expanded(&path) {
            if collapse {
                self.inner.tree.borrow_mut().collapse(&path);
                self.publish();
            } else {
                self.move_by(1);
            }
            return None;
        }
        self.with_tree_on_a_worker(move |tree| tree.expand(&path));
        None
    }

    /// Closes the selected directory, or goes to the one holding the selected row.
    pub fn parent_or_close(&self) {
        let Some(row) = self.selected() else {
            return;
        };
        let path = row.entry.path.clone();
        if row.entry.directory && row.expanded {
            self.inner.tree.borrow_mut().collapse(&path);
            self.publish();
            return;
        }
        let Some(parent) = path.parent().map(Path::to_path_buf) else {
            return;
        };
        if parent == self.root() {
            self.go_to(0);
            return;
        }
        self.go_to_path(&parent);
    }

    /// Reads everything that is open again.
    ///
    /// What is open stays open, and the caret stays on the row it was on rather than on the index
    /// it was at: a file made above it moves every index below, and a caret that slid down a
    /// screen is one nobody asked for.
    pub fn refresh(&self) {
        self.inner.stale.set(false);
        let at = self.selected().map(|row| row.entry.path);
        self.with_tree_then(zdt_core::tree::Tree::refresh, move |explorer| {
            if let Some(at) = at.as_deref() {
                explorer.go_to_path(at);
            }
        });
    }

    /// The same, when the panel is open. A closed panel remembers instead.
    pub fn refresh_if_open(&self) {
        if self.is_open_untracked() {
            self.refresh();
        } else {
            self.inner.stale.set(true);
        }
    }

    /// Opens the way to `path` and puts the caret on it.
    pub fn reveal(&self, path: &Path) {
        let path = path.to_path_buf();
        let landing = path.clone();
        self.with_tree_then(
            move |tree| tree.reveal(&path),
            move |explorer| explorer.go_to_path(&landing),
        );
    }

    /// Changes what the tree shows.
    pub fn set_filter(&self, filter: Filter) {
        self.with_tree_on_a_worker(move |tree| {
            tree.set_filter(filter);
            tree.refresh();
        });
    }

    /// What the tree is showing and leaving out.
    #[must_use]
    pub fn filter(&self) -> Filter {
        self.inner.tree.borrow().filter()
    }

    // ---- The clipboard ---------------------------------------------------------------------

    /// Remembers the selected row for a later paste.
    pub fn hold(&self, cut: bool) -> Option<PathBuf> {
        let row = self.selected()?;
        self.inner.clipboard.set(Some(Clipboard {
            path: row.entry.path.clone(),
            cut,
        }));
        Some(row.entry.path)
    }

    /// Forgets what was being held.
    pub fn release(&self) {
        if self.inner.clipboard.get_untracked().is_some() {
            self.inner.clipboard.set(None);
        }
    }

    // ---- Internals ---------------------------------------------------------------------------

    /// Runs `work` against the tree on a worker, then publishes the rows.
    fn with_tree_on_a_worker(&self, work: impl FnOnce(&mut Tree) + Send + 'static) {
        self.with_tree_then(work, |_| {});
    }

    /// Whether the panel is open, without subscribing.
    #[must_use]
    pub fn is_open_untracked(&self) -> bool {
        self.inner.open.get_untracked()
    }

    /// Which directories are open, and which row the caret is on, for a session to write down.
    ///
    /// The caret by *path* and never by index: a directory opening above it moves every index
    /// below, so an index restored into a differently-expanded tree lands somewhere else.
    #[must_use]
    pub fn session_state(&self) -> (Vec<PathBuf>, Option<PathBuf>, Vec<PathBuf>) {
        let tree = self.inner.tree.borrow();
        let at = tree
            .rows()
            .get(self.inner.at.get_untracked())
            .map(|row| row.entry.path.clone());
        (tree.expanded(), at, self.marked())
    }

    /// Puts back what [`session_state`](Self::session_state) took.
    ///
    /// One worker hop for the whole set, and the caret afterwards, because where a path is in the
    /// list depends on what was expanded before it.
    pub fn restore_session(
        &self,
        expanded: Vec<PathBuf>,
        at: Option<PathBuf>,
        marked: Vec<PathBuf>,
    ) {
        self.inner.marked.set(marked);
        self.with_tree_then(
            move |tree| tree.restore_expanded(&expanded),
            move |explorer| {
                if let Some(at) = at.as_ref() {
                    explorer.go_to_path(at);
                }
            },
        );
    }

    /// The same, and then `after` on the interface thread once the rows are published.
    ///
    /// The tree is taken out, worked on elsewhere and put back, because reading a directory is
    /// blocking and the interface thread must not wait on one.
    fn with_tree_then(
        &self,
        work: impl FnOnce(&mut Tree) + Send + 'static,
        after: impl FnOnce(&Explorer) + 'static,
    ) {
        let root = self.root();
        let filter = self.filter();
        let taken = std::mem::replace(&mut *self.inner.tree.borrow_mut(), Tree::new(root, filter));

        let explorer = self.clone();
        // Detached: a key pressed in the tree can be what closes the tree, and a read cancelled
        // half way would leave the model holding an empty tree it had already taken apart.
        zdt_view::detached(async move {
            let tree = zgui::task::blocking(move || {
                let mut tree = taken;
                work(&mut tree);
                tree
            })
            .await;

            *explorer.inner.tree.borrow_mut() = tree;
            explorer.publish();
            after(&explorer);
        });
    }

    /// Puts the rows where the list reads them, keeping the caret in range.
    pub(super) fn publish(&self) {
        let rows = self.inner.tree.borrow().rows();
        let count = rows.len();
        self.inner.rows.set(rows);
        let at = self.inner.at.get_untracked();
        if count == 0 {
            self.inner.at.set(0);
        } else if at >= count {
            self.inner.at.set(count - 1);
        }
    }
}

/// Puts the explorer where every component can find it.
pub fn provide(explorer: Explorer) {
    zgui::reactive::provide_local_context(explorer);
}

/// The explorer, from inside a component.
///
/// # Panics
///
/// If none was provided above this component. That is a wiring mistake, and nothing can carry on
/// from it.
#[must_use]
pub fn use_explorer() -> Explorer {
    zgui::reactive::use_local_context::<Explorer>().expect("an explorer is provided at the root")
}
