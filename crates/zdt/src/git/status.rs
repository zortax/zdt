//! What git says about every path in the tree.
//!
//! One `zdt_git::status` on a worker, rolled up onto the directories above each changed file, and
//! read one row at a time while the tree draws.
//!
//! Separate from [`Git`](super::Git), which answers which *lines* of an open buffer changed. That
//! one follows the buffers and is read by a gutter; this one follows the whole working tree and is
//! read by the file tree. Different sources, different lifetimes, different moments to run again.
//!
//! A status reads the whole working tree, so it runs only while the tree is open. A session with
//! the panel closed does none of this work. What says the tree moved is [`crate::disk`].

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use rustc_hash::FxHashMap;
use zdt_git::{Repo, State};
use zdt_icons as icons;
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

/// How long to wait before reading again, so a run of changes is one read.
const SETTLE: Duration = Duration::from_millis(150);

/// What one row of the tree shows about git.
///
/// One state and not two. A row is an outline and a tone, and the panel is where both halves of a
/// file's status are shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mark {
    /// What has happened to it.
    pub state: State,
    /// Whether what it shows is in the index.
    pub staged: bool,
    /// Whether it stands for something inside it, and not for the row itself.
    pub rolled: bool,
}

impl Mark {
    /// What git says about `entry`, as the one thing a row shows.
    ///
    /// A conflict first, because it is the one that has to be answered. Then the working tree,
    /// which is the half that is *not* going into the commit and so the half worth a mark. The
    /// index last.
    #[must_use]
    fn of(entry: &zdt_git::Entry) -> Option<Self> {
        if entry.is_conflicted() {
            return Some(Self {
                state: State::Conflicted,
                staged: false,
                rolled: false,
            });
        }
        if entry.worktree.is_change() {
            return Some(Self {
                state: entry.worktree,
                staged: false,
                rolled: false,
            });
        }
        if entry.index.is_change() {
            return Some(Self {
                state: entry.index,
                staged: true,
                rolled: false,
            });
        }
        None
    }

    /// The outline it draws.
    #[must_use]
    pub const fn icon(self) -> &'static str {
        if self.rolled {
            // A directory says only that something under it changed. Which file, and what
            // happened to it, is one row further in.
            return icons::DOT;
        }
        match self.state {
            State::Conflicted => icons::TRIANGLE_ALERT,
            State::Untracked => icons::PLUS,
            State::Deleted => icons::MINUS,
            State::Renamed => icons::ARROW_RIGHT,
            _ if self.staged => icons::CIRCLE_CHECK,
            _ => icons::PENCIL,
        }
    }

    /// The colour it draws in.
    #[must_use]
    pub const fn tint(self) -> &'static str {
        match self.state {
            State::Conflicted => "zdt-git-conflict",
            State::Untracked => "zdt-git-untracked",
            State::Deleted => "zdt-git-removed",
            State::Added if !self.staged => "zdt-git-added",
            _ if self.staged => "zdt-git-added",
            _ => "zdt-git-changed",
        }
    }

    /// How loudly it asks to be noticed.
    ///
    /// What decides which of several marks a directory shows: the one that would make somebody
    /// open the directory.
    const fn weight(self) -> u8 {
        match self.state {
            State::Conflicted => 4,
            State::Untracked => 1,
            _ if self.staged => 2,
            _ => 3,
        }
    }

    /// The same mark, standing for something inside a directory.
    const fn rolled(self) -> Self {
        Self {
            rolled: true,
            ..self
        }
    }
}

/// What git says about every path in the tree.
///
/// Cloning one is cloning a handle.
#[derive(Clone)]
pub struct Status {
    inner: Rc<Inner>,
}

struct Inner {
    /// Where the tree is rooted. A mark outside it has no row to sit on.
    root: PathBuf,
    /// The clock the reads are debounced on, lent by whichever window is attached.
    clock: zdt_view::Clock,
    /// The repository the root is in, when it is in one.
    repo: Option<Repo>,
    /// Whether anything is looking.
    wanted: Cell<bool>,
    /// What git says, by path, with the directories above them rolled in.
    ///
    /// A signal over an `Rc`, so a row subscribes by reading it and a publish is one pointer.
    marks: RwSignal<Rc<FxHashMap<PathBuf, Mark>>, LocalStorage>,
    /// What is waiting to read.
    pending: RefCell<Option<zdt_view::Pending>>,
    /// Which read is the current one, so a slow answer to an old question is dropped.
    generation: Cell<u64>,
}

impl Status {
    /// Nothing known yet, for a tree rooted at `root`.
    ///
    /// The repository is looked for once, here. A root that is in none answers nothing, and every
    /// method costs nothing from then on.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>, clock: zdt_view::Clock) -> Self {
        let root = root.into();
        let repo = Repo::open(&root).ok();
        Self {
            inner: Rc::new(Inner {
                root,
                clock,
                repo,
                wanted: Cell::new(false),
                marks: RwSignal::new_local(Rc::new(FxHashMap::default())),
                pending: RefCell::new(None),
                generation: Cell::new(0),
            }),
        }
    }

    /// Whether the tree is in a repository at all.
    #[must_use]
    pub fn is_repository(&self) -> bool {
        self.inner.repo.is_some()
    }

    /// What the row for `path` shows. Tracked.
    #[must_use]
    pub fn mark(&self, path: &Path) -> Option<Mark> {
        self.inner.marks.with(|marks| {
            if let Some(found) = marks.get(path) {
                return Some(*found);
            }
            // Git names an untracked directory once and never what is inside it, so a row under
            // one takes the same mark. Everything else is answered by the map alone.
            let mut at = path;
            while let Some(parent) = at.parent() {
                if !parent.starts_with(&self.inner.root) {
                    break;
                }
                if let Some(found) = marks.get(parent)
                    && !found.rolled
                    && matches!(found.state, State::Untracked)
                {
                    return Some(Mark {
                        rolled: false,
                        ..*found
                    });
                }
                at = parent;
            }
            None
        })
    }

    /// Says whether anything is looking at the marks.
    ///
    /// Turning it on reads at once. Turning it off drops whatever was waiting to be read, and
    /// every later question is answered by doing nothing until it is turned on again.
    ///
    /// What says the marks are out of date is [`crate::disk`], which watches the project and the
    /// repository for the whole session. This only decides whether to act on what it says.
    pub fn watch(&self, wanted: bool) {
        if self.inner.wanted.replace(wanted) == wanted {
            return;
        }
        if wanted {
            self.refresh();
        } else {
            self.inner.pending.borrow_mut().take();
        }
    }

    /// Reads the status again, now.
    pub fn refresh(&self) {
        self.inner.pending.borrow_mut().take();
        if !self.inner.wanted.get() {
            return;
        }
        let Some(repo) = self.inner.repo.clone() else {
            return;
        };
        let generation = self.inner.generation.get().wrapping_add(1);
        self.inner.generation.set(generation);

        let status = self.clone();
        // Detached: closing the tree is what stops the watching, and a read cancelled half way
        // would leave the marks saying what was true two commits ago.
        zdt_view::detached(async move {
            let found =
                zgui::task::blocking(move || zdt_git::status::status(&repo).unwrap_or_default())
                    .await;
            // An answer to a question nobody is asking any more.
            if status.inner.generation.get() != generation {
                return;
            }
            let built = collect(&found, &status.inner.root);
            // A watch on `.git` fires for every write to the index, and most of those say the
            // same thing twice.
            if status.inner.marks.with_untracked(|held| **held != built) {
                status.inner.marks.set(Rc::new(built));
            }
        });
    }

    /// Reads it again shortly, so that a run of changes is one read.
    pub fn refresh_soon(&self) {
        if !self.inner.wanted.get() {
            return;
        }
        let status = self.clone();
        let handle = self.inner.clock.after(SETTLE, move || {
            status.inner.pending.borrow_mut().take();
            status.refresh();
        });
        *self.inner.pending.borrow_mut() = Some(handle);
    }
}

/// Every path git named, and every directory above them, as one map.
///
/// Kept inside `root`: a repository can enclose the directory the tree is rooted at, and a mark
/// for a path outside the tree has no row to sit on.
///
/// Two passes. The own marks go in first, so a directory git named in its own right — a fresh
/// `target/`, which git reports once instead of nine thousand times — keeps the mark it earned
/// and is never overwritten by a rollup from below it.
fn collect(entries: &[zdt_git::Entry], root: &Path) -> FxHashMap<PathBuf, Mark> {
    let mut marks: FxHashMap<PathBuf, Mark> = FxHashMap::default();

    let own: Vec<(PathBuf, Mark)> = entries
        .iter()
        .filter(|entry| entry.full.starts_with(root))
        .filter_map(|entry| Mark::of(entry).map(|mark| (entry.full.clone(), mark)))
        .collect();
    for (path, mark) in &own {
        marks.insert(path.clone(), *mark);
    }

    for (path, mark) in &own {
        let rolled = mark.rolled();
        let mut at = path.as_path();
        while let Some(parent) = at.parent() {
            // Up to the root and never onto it. The root is the panel's header rather than a row,
            // so a mark there has nothing to draw itself on.
            if parent == root || !parent.starts_with(root) {
                break;
            }
            match marks.get(parent) {
                // A directory that stands for itself outweighs anything under it.
                Some(held) if !held.rolled => break,
                Some(held) if held.weight() >= rolled.weight() => {}
                _ => {
                    marks.insert(parent.to_path_buf(), rolled);
                }
            }
            at = parent;
        }
    }

    marks
}

/// Puts the marks where every component can find them.
pub fn provide(status: Status) {
    zgui::reactive::provide_local_context(status);
}

/// Them, from inside a component.
///
/// # Panics
///
/// If none were provided above this component, which is a wiring mistake.
#[must_use]
pub fn use_status() -> Status {
    zgui::reactive::use_local_context::<Status>().expect("a status layer is provided at the root")
}

#[cfg(test)]
mod tests {
    use super::{Mark, collect};
    use std::path::{Path, PathBuf};
    use zdt_git::State;

    /// One entry git would report.
    fn entry(path: &str, index: State, worktree: State) -> zdt_git::Entry {
        zdt_git::Entry {
            full: PathBuf::from("/work").join(path),
            path: path.to_owned(),
            index,
            worktree,
            from: None,
        }
    }

    /// The root every test here uses.
    fn root() -> &'static Path {
        Path::new("/work")
    }

    #[test]
    fn the_working_tree_is_the_half_worth_a_mark() {
        // Staged and changed again since: the half that is not going into the commit is the one
        // somebody needs to be told about.
        let both = entry("a.rs", State::Modified, State::Modified);
        let mark = Mark::of(&both).expect("it is a change");
        assert_eq!(mark.state, State::Modified);
        assert!(!mark.staged);

        let staged = entry("b.rs", State::Modified, State::Unchanged);
        assert!(Mark::of(&staged).expect("it is a change").staged);

        assert!(Mark::of(&entry("c.rs", State::Unchanged, State::Unchanged)).is_none());
    }

    #[test]
    fn a_conflict_outranks_everything_on_the_row() {
        let mark = Mark::of(&entry("a.rs", State::Modified, State::Conflicted));
        assert_eq!(mark.expect("it is a change").state, State::Conflicted);
    }

    #[test]
    fn a_directory_takes_the_loudest_mark_under_it() {
        // A new file and a modified one: the modification is what a person wants to see first.
        let found = [
            entry("src/new.rs", State::Unchanged, State::Untracked),
            entry("src/old.rs", State::Unchanged, State::Modified),
        ];
        let marks = collect(&found, root());

        let directory = marks.get(Path::new("/work/src")).expect("it is rolled up");
        assert!(directory.rolled);
        assert_eq!(directory.state, State::Modified);
    }

    #[test]
    fn a_conflict_reaches_every_directory_above_it() {
        let found = [
            entry("a/b/bad.rs", State::Unchanged, State::Conflicted),
            entry("a/other.rs", State::Modified, State::Unchanged),
        ];
        let marks = collect(&found, root());

        for directory in ["/work/a", "/work/a/b"] {
            let mark = marks.get(Path::new(directory)).expect("it is rolled up");
            assert_eq!(mark.state, State::Conflicted, "{directory}");
        }
    }

    #[test]
    fn a_directory_git_named_itself_keeps_its_own_mark() {
        // Git reports a fresh `target/` once and never what is inside it.
        let found = [entry("target", State::Unchanged, State::Untracked)];
        let marks = collect(&found, root());

        let mark = marks.get(Path::new("/work/target")).expect("it is there");
        assert!(!mark.rolled, "it stands for itself");
        assert_eq!(mark.state, State::Untracked);
    }

    #[test]
    fn a_path_outside_the_tree_has_no_row_to_sit_on() {
        // The repository can enclose the directory the tree is rooted at.
        let mut outside = entry("elsewhere.rs", State::Unchanged, State::Modified);
        outside.full = PathBuf::from("/elsewhere.rs");
        let marks = collect(&[outside], root());
        assert!(marks.is_empty());
    }

    #[test]
    fn the_root_itself_is_never_marked() {
        // It is the header and not a row, so a mark on it has nothing to draw itself on.
        let found = [entry("src/a.rs", State::Unchanged, State::Modified)];
        let marks = collect(&found, root());
        assert!(marks.contains_key(Path::new("/work/src")));
        assert!(!marks.contains_key(root()));
    }
}
