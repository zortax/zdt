//! The file tree's state, as the interface reads it.
//!
//! The tree itself is [`zdt_core::tree`] and knows nothing about signals. This is what makes it
//! reactive, plus the two things a tree has that a list of rows does not: which row the caret is
//! on, and what is waiting to be pasted.
//!
//! Reading a directory is blocking, so every operation that reads one goes through a worker and
//! writes the answer back on the interface thread.

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
    tree: RefCell<Tree>,
    /// The rows, as the list draws them.
    rows: RwSignal<Vec<Row>, LocalStorage>,
    /// Which row the caret is on.
    at: RwSignal<usize, LocalStorage>,
    /// Whether the panel is shown at all.
    open: RwSignal<bool, LocalStorage>,
    /// Whether the keyboard is in it.
    focused: RwSignal<bool, LocalStorage>,
    /// How many times the keyboard has been asked for.
    ///
    /// Separate from `focused`, because asking twice in a row is a real request: the prompt takes
    /// the keyboard away without the panel ever saying it had lost it, so the panel has to be able
    /// to ask for it back while still believing it has it.
    claims: RwSignal<u64, LocalStorage>,
    /// What a cut or a copy left waiting.
    clipboard: RwSignal<Option<Clipboard>, LocalStorage>,
    /// Every row a person has picked out, beside the one the caret is on.
    ///
    /// By path, and never by index. A directory opening above them moves every index below it,
    /// and a selection that slid down a screen is one nobody meant.
    marked: RwSignal<Vec<PathBuf>, LocalStorage>,
    /// What is being dragged, and where it would land.
    dragging: RwSignal<Option<PathBuf>, LocalStorage>,
    over: RwSignal<Option<PathBuf>, LocalStorage>,
}

impl Explorer {
    /// A tree over `root`, closed.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>, filter: Filter) -> Self {
        Self {
            inner: Rc::new(Inner {
                tree: RefCell::new(Tree::new(root, filter)),
                rows: RwSignal::new_local(Vec::new()),
                at: RwSignal::new_local(0),
                open: RwSignal::new_local(false),
                focused: RwSignal::new_local(false),
                claims: RwSignal::new_local(0),
                clipboard: RwSignal::new_local(None),
                marked: RwSignal::new_local(Vec::new()),
                dragging: RwSignal::new_local(None),
                over: RwSignal::new_local(None),
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

    /// Whether the panel is shown. Tracked.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.inner.open.get()
    }

    /// Whether the keyboard is in it. Tracked.
    #[must_use]
    pub fn is_focused(&self) -> bool {
        self.inner.focused.get()
    }

    /// How many times the keyboard has been asked for. Tracked.
    ///
    /// What the panel watches to know it should take the keyboard, so that asking again after
    /// something else borrowed it works.
    #[must_use]
    pub fn claims(&self) -> u64 {
        self.inner.claims.get()
    }

    /// Whether the keyboard is in it, without subscribing.
    #[must_use]
    pub fn is_focused_untracked(&self) -> bool {
        self.inner.focused.get_untracked()
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

    /// Says a drag of the row at `at` has begun.
    pub fn start_drag(&self, at: usize) {
        if let Some(row) = self.row_at(at) {
            self.inner.dragging.set(Some(row.entry.path));
        }
    }

    /// What is being dragged. Tracked.
    #[must_use]
    pub fn dragging(&self) -> Option<PathBuf> {
        self.inner.dragging.get()
    }

    /// Says the pointer is over the row at `at` during a drag.
    pub fn drag_over(&self, at: usize) {
        if self.inner.dragging.with_untracked(Option::is_none) {
            return;
        }
        let over = self.row_at(at).map(|row| row.entry.path);
        if self.inner.over.get_untracked() != over {
            self.inner.over.set(over);
        }
    }

    /// Which row a drop would land on. Tracked.
    #[must_use]
    pub fn drop_target(&self) -> Option<PathBuf> {
        self.inner.over.get()
    }

    /// Ends the drag, answering what should move where.
    ///
    /// The directory a drop lands in: the row itself when it is one, and the one holding it when
    /// it is a file. That is what dropping *beside* something means.
    pub fn finish_drag(&self) -> Option<(PathBuf, PathBuf)> {
        let from = self.inner.dragging.get_untracked()?;
        let onto = self.inner.over.get_untracked();
        self.cancel_drag();

        let onto = onto?;
        let into = if self.inner.tree.borrow().is_directory(&onto) {
            onto
        } else {
            onto.parent()?.to_path_buf()
        };
        // Onto itself, or into the directory it is already in: nothing to do.
        if into == from || from.parent() == Some(into.as_path()) {
            return None;
        }
        // A directory cannot be moved inside itself, which would take the tree with it.
        if into.starts_with(&from) {
            return None;
        }
        Some((from, into))
    }

    /// Ends the drag without moving anything.
    pub fn cancel_drag(&self) {
        if self.inner.dragging.with_untracked(Option::is_some) {
            self.inner.dragging.set(None);
        }
        if self.inner.over.with_untracked(Option::is_some) {
            self.inner.over.set(None);
        }
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

    /// Shows or hides the panel.
    ///
    /// Opening it for the first time reads the root, which is why it takes a worker.
    pub fn toggle(&self) {
        let open = !self.inner.open.get_untracked();
        self.inner.open.set(open);
        if open {
            self.inner.focused.set(true);
            self.inner.claims.update(|claims| *claims += 1);
            if self.inner.rows.with_untracked(Vec::is_empty) {
                self.expand_root();
            }
        } else {
            self.inner.focused.set(false);
        }
    }

    /// Hides the panel.
    pub fn close(&self) {
        self.inner.open.set(false);
        self.inner.focused.set(false);
    }

    /// Gives the keyboard to the panel, opening it first when it is closed.
    pub fn focus(&self) {
        if !self.inner.open.get_untracked() {
            self.toggle();
            return;
        }
        self.inner.focused.set(true);
        self.inner.claims.update(|claims| *claims += 1);
    }

    /// Takes the keyboard away, leaving the panel where it is.
    pub fn unfocus(&self) {
        if self.inner.focused.get_untracked() {
            self.inner.focused.set(false);
        }
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
    pub fn refresh(&self) {
        self.with_tree_on_a_worker(zdt_core::tree::Tree::refresh);
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
