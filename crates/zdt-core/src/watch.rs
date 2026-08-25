//! Watching the directories of a project.
//!
//! One watch per directory, and never one recursive watch over the root. A recursive watch puts a
//! kernel watch on every directory under it, which for a Rust project is every directory in
//! `target/`: tens of thousands of watches, and an event for each of the thousands of files a
//! build writes. So the directories are chosen here, by the same ignore files the tree reads, and
//! what a build writes is never watched at all.
//!
//! What is reported is one [`Change`] per event, from the watcher's own thread. Collecting them
//! into one refresh belongs to the caller, which is the only part that knows what a refresh costs.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};

/// What happened to a path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// It was written to. The tree has the same shape as before.
    Written,
    /// It was made, removed or renamed, so the shape of the tree moved.
    Moved,
}

/// One thing that happened on disk.
#[derive(Clone, Debug)]
pub struct Change {
    /// What it happened to.
    pub path: PathBuf,
    /// What happened.
    pub kind: Kind,
}

/// A watch over the directories of a project.
///
/// Held in an `Arc` because [`sync`](Watch::sync) walks the project and belongs on a worker, while
/// whoever holds the watch is on the interface thread. Dropping the last handle stops the watching.
pub struct Watch {
    /// The top of the project.
    root: PathBuf,
    /// The watcher itself. Behind a lock, because a sync runs on a worker.
    watcher: Mutex<RecommendedWatcher>,
    /// Which directories of the project are watched, so a sync only adds what is new.
    watched: Mutex<BTreeSet<PathBuf>>,
}

impl Watch {
    /// Watches `root` itself, and calls `report` for everything that happens under what is
    /// watched.
    ///
    /// Only the root to begin with. [`sync`](Watch::sync) is what reaches the rest of the project,
    /// and it walks, so it belongs on a worker. A root that cannot be watched answers `None`.
    #[must_use]
    pub fn over(
        root: impl Into<PathBuf>,
        report: impl Fn(Change) + Send + 'static,
    ) -> Option<Arc<Self>> {
        let root = root.into();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                let Ok(event) = event else {
                    return;
                };
                let Some(kind) = kind_of(event.kind) else {
                    return;
                };
                for path in event.paths {
                    report(Change { path, kind });
                }
            })
            .ok()?;

        watcher.watch(&root, RecursiveMode::NonRecursive).ok()?;

        Some(Arc::new(Self {
            root: root.clone(),
            watcher: Mutex::new(watcher),
            watched: Mutex::new(BTreeSet::from([root])),
        }))
    }

    /// Watches `directory` and everything under it.
    ///
    /// For a directory the project's own walk leaves out and something still has to hear about:
    /// `.git`, whose every write is a commit, a stage or a checkout. Small enough that watching it
    /// whole costs nothing.
    pub fn whole(&self, directory: &Path) {
        let Ok(mut watcher) = self.watcher.lock() else {
            return;
        };
        let _ = watcher.watch(directory, RecursiveMode::Recursive);
    }

    /// Watches every directory of the project that is not ignored, and stops watching the ones
    /// that went away.
    ///
    /// Blocking: it walks the project. Called once when the watch is made, and again whenever
    /// something was made or renamed, because a directory nobody watches is a directory whose
    /// files change silently.
    pub fn sync(&self) {
        let wanted = directories(&self.root);
        let (Ok(mut watcher), Ok(mut watched)) = (self.watcher.lock(), self.watched.lock()) else {
            return;
        };

        for directory in wanted.difference(&watched) {
            let _ = watcher.watch(directory, RecursiveMode::NonRecursive);
        }
        // A directory that has been removed already took its own watch with it, and saying so
        // twice is an error nobody can act on.
        for directory in watched.difference(&wanted) {
            let _ = watcher.unwatch(directory);
        }
        *watched = wanted;
    }
}

/// What one event kind means here, when it means anything.
///
/// A read means nothing: opening a file to search it must not look like somebody editing it.
const fn kind_of(kind: notify::EventKind) -> Option<Kind> {
    use notify::EventKind::{Access, Any, Create, Modify, Other, Remove};
    use notify::event::ModifyKind;

    match kind {
        Access(_) => None,
        Create(_) | Remove(_) | Modify(ModifyKind::Name(_)) => Some(Kind::Moved),
        Modify(_) | Any | Other => Some(Kind::Written),
    }
}

/// Every directory of the project worth watching.
///
/// The ignore files decide, as they do for the tree. What a build writes is left out even when the
/// tree is showing it: a watch on `target/` is tens of thousands of kernel watches and an event
/// for every file a build touches, which is the one thing this whole module exists to avoid.
///
/// `.git` is left out too. It is watched whole by [`Watch::whole`], because git replaces the files
/// in it rather than writing into them.
fn directories(root: &Path) -> BTreeSet<PathBuf> {
    let mut walk = ignore::WalkBuilder::new(root);
    walk.hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .ignore(true)
        .parents(true)
        // A `.gitignore` says what to leave out whether or not there is a `.git` beside it.
        .require_git(false)
        // A link is a file here, so a loop of them is not a walk with no bottom.
        .follow_links(false)
        .filter_entry(|entry| {
            entry.file_type().is_some_and(|kind| kind.is_dir()) && entry.file_name() != ".git"
        });

    walk.build()
        .filter_map(Result::ok)
        .map(ignore::DirEntry::into_path)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Kind, directories, kind_of};

    /// A project with an ignored build directory and a `.git` in it.
    fn sample(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("zdt-watch-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src/inner")).expect("made");
        std::fs::create_dir_all(root.join("target/debug/deps")).expect("made");
        std::fs::create_dir_all(root.join(".git/objects")).expect("made");
        std::fs::write(root.join(".gitignore"), "target\n").expect("written");
        std::fs::write(root.join("src/main.rs"), "").expect("written");
        root
    }

    #[test]
    fn what_a_build_writes_is_never_watched() {
        // The whole reason the directories are chosen rather than walked by the kernel.
        let root = sample("ignored");
        let found = directories(&root);

        assert!(found.contains(&root));
        assert!(found.contains(&root.join("src")));
        assert!(found.contains(&root.join("src/inner")));
        assert!(!found.contains(&root.join("target")));
        assert!(!found.contains(&root.join("target/debug/deps")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_repository_is_left_to_the_watch_that_takes_it_whole() {
        let root = sample("dot-git");
        let found = directories(&root);

        assert!(!found.contains(&root.join(".git")));
        assert!(!found.contains(&root.join(".git/objects")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn only_directories_are_watched() {
        // A watch on a file follows the file that was renamed away when it is saved.
        let root = sample("files");
        assert!(!directories(&root).contains(&root.join("src/main.rs")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_read_is_not_a_change() {
        use notify::EventKind;
        use notify::event::{AccessKind, CreateKind, DataChange, ModifyKind, RenameMode};

        assert_eq!(kind_of(EventKind::Access(AccessKind::Read)), None);
        assert_eq!(
            kind_of(EventKind::Modify(ModifyKind::Data(DataChange::Content))),
            Some(Kind::Written)
        );
        assert_eq!(
            kind_of(EventKind::Create(CreateKind::File)),
            Some(Kind::Moved)
        );
        assert_eq!(
            kind_of(EventKind::Modify(ModifyKind::Name(RenameMode::Both))),
            Some(Kind::Moved)
        );
    }
}
