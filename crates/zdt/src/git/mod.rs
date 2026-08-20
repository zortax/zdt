//! The git signs, as the interface keeps them.
//!
//! One diff per file, run on a worker when the file is opened and again a moment after it is
//! saved. Never on a keystroke. `git diff` reads the index and the object store, and doing that
//! between two characters is a process spawn per keystroke for a gutter mark that is only ever
//! read at rest.
//!
//! The marks go into the editor's `"git"` decoration layer, beside the diagnostics' `"lsp"` one.
//! That is the whole reason layers are named.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use rustc_hash::FxHashMap;
use zdt_git::{Change, Hunk};
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

mod host;
pub mod status;

pub use crate::git::host::panel;
pub use crate::git::status::{Mark, Status, use_status};

use crate::workspace::{BufferId, Workspace};

/// How long after a save the diff is run again.
///
/// A moment, so that a formatter or a hook that rewrites the file on save has finished before the
/// signs are worked out from it.
const AFTER_SAVE: Duration = Duration::from_millis(200);

/// What git says about every open file.
#[derive(Clone)]
pub struct Git {
    inner: Rc<Inner>,
}

struct Inner {
    workspace: Workspace,
    /// The clock the after-save refresh is debounced on, lent by whichever window is attached.
    clock: zdt_view::Clock,
    /// The hunks in each file.
    hunks: RefCell<FxHashMap<PathBuf, Vec<Hunk>>>,
    /// What is waiting to run a diff, by file.
    pending: RefCell<FxHashMap<PathBuf, zdt_view::Pending>>,
    /// A number that changes whenever the hunks have.
    revision: RwSignal<u64, LocalStorage>,
}

impl Git {
    /// Nothing known yet.
    #[must_use]
    pub fn new(workspace: Workspace, clock: zdt_view::Clock) -> Self {
        Self {
            inner: Rc::new(Inner {
                workspace,
                clock,
                hunks: RefCell::new(FxHashMap::default()),
                pending: RefCell::new(FxHashMap::default()),
                revision: RwSignal::new_local(0),
            }),
        }
    }

    /// A number that changes whenever the signs have. Tracked.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.inner.revision.get()
    }

    /// What has changed in `path`.
    #[must_use]
    pub fn hunks(&self, path: &Path) -> Vec<Hunk> {
        self.inner
            .hunks
            .borrow()
            .get(path)
            .cloned()
            .unwrap_or_default()
    }

    /// Runs the diff for `buffer`, now.
    pub fn refresh(&self, buffer: BufferId) {
        let Some(path) = self
            .inner
            .workspace
            .buffer_untracked(buffer)
            .and_then(|entry| entry.path)
        else {
            return;
        };
        self.refresh_path(&path);
    }

    /// The same, for a path.
    pub fn refresh_path(&self, path: &Path) {
        let path = path.to_path_buf();
        let git = self.clone();
        zdt_view::detached(async move {
            let reading = path.clone();
            let found = zgui::task::blocking(move || zdt_git::hunks(&reading)).await;

            let changed = git
                .inner
                .hunks
                .borrow()
                .get(&path)
                .is_none_or(|held| *held != found);
            if !changed {
                return;
            }
            if found.is_empty() {
                git.inner.hunks.borrow_mut().remove(&path);
            } else {
                git.inner.hunks.borrow_mut().insert(path, found);
            }
            git.inner
                .revision
                .update(|revision| *revision = revision.wrapping_add(1));
        });
    }

    /// Runs it again shortly, which saving does.
    pub fn refresh_soon(&self, buffer: BufferId) {
        let Some(path) = self
            .inner
            .workspace
            .buffer_untracked(buffer)
            .and_then(|entry| entry.path)
        else {
            return;
        };
        let git = self.clone();
        let waiting = path.clone();
        let handle = self.inner.clock.after(AFTER_SAVE, move || {
            git.inner.pending.borrow_mut().remove(&waiting);
            git.refresh_path(&waiting);
        });
        self.inner.pending.borrow_mut().insert(path, handle);
    }

    /// Forgets a file, which closing it does.
    pub fn forget(&self, path: &Path) {
        self.inner.hunks.borrow_mut().remove(path);
        self.inner.pending.borrow_mut().remove(path);
    }
}

/// Which glyph a change gets in the gutter.
#[must_use]
pub const fn glyph(change: Change) -> &'static str {
    match change {
        Change::Added => "\u{2502}",
        Change::Changed => "\u{2502}",
        // A removal has no lines of its own, so it is a wedge pointing at where they were rather
        // than a bar beside lines that are still here.
        Change::Removed => "\u{25b8}",
    }
}

/// Which colour it is drawn in.
#[must_use]
pub const fn tint(change: Change) -> &'static str {
    match change {
        Change::Added => "zdt-git-added",
        Change::Changed => "zdt-git-changed",
        Change::Removed => "zdt-git-removed",
    }
}

/// Puts the git layer where every component can find it.
pub fn provide(git: Git) {
    zgui::reactive::provide_local_context(git);
}

/// It, from inside a component.
///
/// # Panics
///
/// If none was provided above this component, which is a wiring mistake.
#[must_use]
pub fn use_git() -> Git {
    zgui::reactive::use_local_context::<Git>().expect("a git layer is provided at the root")
}
