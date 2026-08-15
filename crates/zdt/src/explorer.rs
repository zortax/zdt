//! The file tree's state, as the interface reads it.
//!
//! The tree itself is [`zdt_core::tree`] and knows nothing about signals. This is what makes it
//! reactive, plus the two things a tree has that a list of rows does not: which row the caret is
//! on, and what is waiting to be pasted.
//!
//! Reading a directory is blocking, so every operation that reads one goes through a worker and
//! writes the answer back on the interface thread.

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
    /// Whether pasting should move it rather than copy it.
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

    /// Opens the selected directory, or opens the selected file.
    ///
    /// Answers the file to open, when the selected row is one — the caller opens it, because the
    /// tree has no business knowing what a buffer is.
    pub fn open_selected(&self) -> Option<PathBuf> {
        let row = self.selected()?;
        if !row.entry.directory {
            return Some(row.entry.path);
        }
        let path = row.entry.path.clone();
        if self.inner.tree.borrow().is_expanded(&path) {
            // Already open: `l` steps into it rather than closing it.
            self.move_by(1);
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
        crate::task::detached(async move {
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
    fn publish(&self) {
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
/// If none was provided above this component, which is a wiring mistake rather than a state
/// anything can carry on from.
#[must_use]
pub fn use_explorer() -> Explorer {
    zgui::reactive::use_local_context::<Explorer>().expect("an explorer is provided at the root")
}
