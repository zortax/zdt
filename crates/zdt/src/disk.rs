//! Keeping the editor in step with the files under it.
//!
//! An agent writes a file, a rebase rewrites forty, a formatter runs in a terminal: the disk moves
//! under the editor all the time, and everything drawn from it has to move with it. The watch is
//! [`zdt_core::watch`], which reports one event per change from its own thread. This is the other
//! half: collecting a run of events into one settled batch, and saying what each part of the
//! editor should read again.
//!
//! What follows the disk:
//!
//! - every open file with no unsaved work, which is re-read from what is now on disk;
//! - the file tree, when something was made, removed or renamed;
//! - what git says about each row of the tree, and the signs in each gutter;
//! - the branch in the corner of the window.
//!
//! A buffer somebody has typed into is never overwritten. What is on disk and what is in the
//! editor disagree, and the editor's save-time handling is where that is settled.
//!
//! The watch covers the project, so a file opened from outside it does not follow the disk. Every
//! path a session works with is inside it.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rustc_hash::FxHashSet;
use zdt_core::watch::{Change, Kind};

use crate::explorer::Explorer;
use crate::git::{Git, Head, Status};
use crate::workspace::Workspace;

/// How long to wait after the first change before anything is read.
///
/// A save writes a temporary and renames it, a build writes a directory full of files, and a
/// checkout rewrites the whole tree. All of them are one read at the end of the wait.
const SETTLE: Duration = Duration::from_millis(120);

/// How many changes are held between one settle and the next.
///
/// A checkout of a large branch overruns this, which is why overrunning it is remembered: what is
/// lost is *which* files moved, and the answer to that is to read them all.
const WAITING: usize = 512;

/// What the editor reads again when the disk moves.
///
/// Cloning one is cloning a handle. Dropping the last one stops the watching.
#[derive(Clone)]
pub struct Disk {
    inner: Rc<Inner>,
}

struct Inner {
    workspace: Workspace,
    explorer: Explorer,
    git: Git,
    status: Status,
    head: Head,
    /// The clock the settle runs on, lent by whichever window is attached.
    clock: zdt_view::Clock,
    /// The watch itself. In an `Arc` because a walk of its directories runs on a worker.
    watch: Arc<zdt_core::watch::Watch>,
    /// Where the repository keeps its own files, when the project is in one.
    dot_git: Option<PathBuf>,
    /// Which files moved since the last read.
    changed: RefCell<FxHashSet<PathBuf>>,
    /// Whether anything was made, removed or renamed, so the tree has a different shape.
    moved: Cell<bool>,
    /// Whether the repository itself moved: a commit, a stage, a checkout.
    repository: Cell<bool>,
    /// Whether more changes arrived than could be held, so which files moved is not known.
    flooded: Arc<AtomicBool>,
    /// The read that is waiting to happen.
    pending: RefCell<Option<zdt_view::Pending>>,
    /// What carries changes from the watch onto the interface thread.
    pump: RefCell<Option<zgui::task::Task>>,
}

impl Disk {
    /// Watches the project under `workspace` and keeps everything named here in step with it.
    ///
    /// A project that cannot be watched answers `None`, and the editor works as it did before a
    /// watch existed: what is on disk is read when something asks for it.
    #[must_use]
    pub fn follow(
        workspace: &Workspace,
        explorer: &Explorer,
        git: &Git,
        status: &Status,
        head: &Head,
        clock: &zdt_view::Clock,
    ) -> Option<Self> {
        let root = workspace.project().root().to_path_buf();

        // A bounded channel, so a checkout that rewrites ten thousand files cannot grow the queue
        // without end. What overruns it is remembered rather than lost.
        let (sender, receiver) = tokio::sync::mpsc::channel::<Change>(WAITING);
        let flooded = Arc::new(AtomicBool::new(false));
        let watch = {
            let flooded = Arc::clone(&flooded);
            zdt_core::watch::Watch::over(&root, move |change| {
                if sender.try_send(change).is_err() {
                    flooded.store(true, Ordering::Relaxed);
                }
            })?
        };

        // The repository's own directory, watched whole: git replaces `HEAD` and the index instead
        // of writing into them, and the project's own walk leaves `.git` out.
        let dot_git = zdt_git::Repo::open(&root).ok().map(|repo| repo.dot_git());
        if let Some(dot_git) = dot_git.as_deref() {
            watch.whole(dot_git);
        }

        let disk = Self {
            inner: Rc::new(Inner {
                workspace: workspace.clone(),
                explorer: explorer.clone(),
                git: git.clone(),
                status: status.clone(),
                head: head.clone(),
                clock: clock.clone(),
                watch,
                dot_git,
                changed: RefCell::new(FxHashSet::default()),
                moved: Cell::new(false),
                repository: Cell::new(false),
                flooded,
                pending: RefCell::new(None),
                pump: RefCell::new(None),
            }),
        };

        // Everything the watch sees, on the interface thread. The pump is held here, so dropping
        // the last handle ends it.
        let pump = zgui::tokio::spawn_receiver(receiver, {
            let disk = disk.clone();
            move |change| disk.saw(&change)
        });
        *disk.inner.pump.borrow_mut() = Some(pump);

        // The rest of the project's directories. A walk, so it belongs on a worker; the root is
        // already watched, so nothing at the top of the project is missed while it runs.
        disk.sync();
        disk.inner.head.refresh();
        Some(disk)
    }

    /// Takes one change from the watch and puts the read off until they stop arriving.
    fn saw(&self, change: &Change) {
        let inner = &self.inner;
        let repository = inner
            .dot_git
            .as_deref()
            .is_some_and(|dot_git| change.path.starts_with(dot_git));

        if repository {
            inner.repository.set(true);
        } else {
            inner.changed.borrow_mut().insert(change.path.clone());
            if change.kind == Kind::Moved {
                inner.moved.set(true);
            }
        }

        let disk = self.clone();
        // Replacing the handle cancels the read that was waiting, which is what makes a run of
        // changes one read at the end of it.
        let waiting = inner.clock.after(SETTLE, move || {
            disk.inner.pending.borrow_mut().take();
            disk.settle();
        });
        *inner.pending.borrow_mut() = Some(waiting);
    }

    /// Reads again whatever the changes since the last settle asked for.
    fn settle(&self) {
        let inner = &self.inner;
        let flooded = inner.flooded.swap(false, Ordering::Relaxed);
        let moved = inner.moved.replace(false) || flooded;
        let repository = inner.repository.replace(false) || flooded;
        let changed = std::mem::take(&mut *inner.changed.borrow_mut());

        if repository {
            // A commit, a stage or a checkout. What the corner says, and what every gutter shows,
            // are both measured against a `HEAD` that has moved.
            inner.head.refresh();
            for buffer in inner.workspace.order_untracked() {
                inner.git.refresh(buffer);
            }
        }

        if repository || !changed.is_empty() {
            // What git says about the tree. It reads the whole working tree, so it runs only
            // while somebody is looking at the panel that draws it.
            inner.status.refresh_soon();
        }

        if moved {
            inner.explorer.refresh_if_open();
            // A directory that nobody watches is a directory whose files change silently, so the
            // watch reaches whatever was just made.
            self.sync();
        }

        if flooded {
            crate::files::refresh(
                &inner.workspace,
                Some(&inner.git),
                crate::files::Which::Everything,
            );
        } else if !changed.is_empty() {
            crate::files::refresh(
                &inner.workspace,
                Some(&inner.git),
                crate::files::Which::These(&changed),
            );
        }
    }

    /// Watches every directory of the project, and stops watching the ones that went away.
    fn sync(&self) {
        let watch = Arc::clone(&self.inner.watch);
        zdt_view::detached(async move {
            zgui::task::blocking(move || watch.sync()).await;
        });
    }
}
