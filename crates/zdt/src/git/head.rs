//! Which branch is checked out, as the window's corner reads it.
//!
//! One small read of the repository, kept in a signal. A commit, a checkout or a rebase moves
//! `HEAD`, and what the header says has to move with it: a branch name that is one checkout out of
//! date is worse than none at all.

use std::path::PathBuf;
use std::rc::Rc;

use zdt_git::Repo;
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

/// Where `HEAD` is, for this session.
///
/// Cloning one is cloning a handle.
#[derive(Clone)]
pub struct Head {
    inner: Rc<Inner>,
}

struct Inner {
    /// Where the repository is looked for.
    root: PathBuf,
    /// What the corner shows: a branch name, or a short commit when the head is detached.
    label: RwSignal<Option<String>, LocalStorage>,
}

impl Head {
    /// A head over the repository at `root`, showing `label` until it is read.
    ///
    /// The label is passed in rather than read here, because the one thing a session already knows
    /// at the moment it is built is which branch it is on, and a corner that fills in one frame
    /// late is a corner that flickers on every start.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>, label: Option<String>) -> Self {
        Self {
            inner: Rc::new(Inner {
                root: root.into(),
                label: RwSignal::new_local(label),
            }),
        }
    }

    /// What the corner shows, when the project is in a repository. Tracked.
    #[must_use]
    pub fn label(&self) -> Option<String> {
        self.inner.label.get()
    }

    /// Reads `HEAD` again.
    ///
    /// On a worker: opening a repository reads files, and the header must not wait on one.
    pub fn refresh(&self) {
        let root = self.inner.root.clone();
        let head = self.clone();
        zdt_view::detached(async move {
            let found = zgui::task::blocking(move || {
                let repo = Repo::open(&root).ok()?;
                zdt_git::head(&repo).ok().map(|head| head.label())
            })
            .await;

            if head.inner.label.get_untracked() != found {
                head.inner.label.set(found);
            }
        });
    }
}

/// Puts it where every component can find it.
pub fn provide(head: Head) {
    zgui::reactive::provide_local_context(head);
}

/// It, from inside a component, when a session provided one.
#[must_use]
pub fn try_use_head() -> Option<Head> {
    zgui::reactive::use_local_context::<Head>()
}
