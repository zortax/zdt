//! Walking a project.
//!
//! One parallel walk, honouring `.gitignore`, producing every path in the project relative to its
//! root. On this machine that is a few milliseconds for a thousand files and well under a second
//! for a hundred thousand — but it is blocking, and belongs on a worker.
//!
//! The paths come back as `String` rather than `PathBuf` because everything downstream of here —
//! the matcher, the list, the preview's header — wants text, and converting once at the edge is
//! cheaper than converting at every use.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// What a walk should and should not look at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Walk {
    /// Whether to include what git ignores.
    pub ignored: bool,
    /// Whether to include names beginning with a dot.
    pub hidden: bool,
    /// Whether to follow symbolic links.
    ///
    /// Off, because a link into a parent directory is a walk that does not end.
    pub follow_links: bool,
    /// How many to stop at, so that a walk started on `/` does not run for ever.
    pub limit: usize,
}

impl Default for Walk {
    fn default() -> Self {
        Self {
            ignored: false,
            hidden: false,
            follow_links: false,
            limit: 500_000,
        }
    }
}

/// Every file under `root`, relative to it.
///
/// Directories are left out: nothing that opens a file wants one, and a project's directories are
/// a large fraction of its entries.
///
/// Blocking. Call it from a worker.
#[must_use]
pub fn walk(root: &Path, options: Walk) -> Vec<String> {
    let found = Mutex::new(Vec::new());
    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .hidden(!options.hidden)
        .follow_links(options.follow_links)
        // Without this, a directory that is not a git repository has its `.gitignore` ignored,
        // which is not what anybody writing one meant.
        .require_git(false)
        .git_ignore(!options.ignored)
        .git_global(!options.ignored)
        .git_exclude(!options.ignored)
        .parents(!options.ignored);

    let root = root.to_path_buf();
    builder.build_parallel().run(|| {
        let found = &found;
        let root = root.clone();
        Box::new(move |entry| {
            let Ok(entry) = entry else {
                return ignore::WalkState::Continue;
            };
            if entry.file_type().is_none_or(|kind| kind.is_dir()) {
                return ignore::WalkState::Continue;
            }
            let path = entry.path();
            let relative = path.strip_prefix(&root).unwrap_or(path);
            let mut found = found
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if found.len() >= options.limit {
                return ignore::WalkState::Quit;
            }
            found.push(relative.to_string_lossy().into_owned());
            ignore::WalkState::Continue
        })
    });

    let mut found = found
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Sorted, so that two runs over the same project give the same order and an unmatched list
    // reads as a tree rather than as whatever order the threads happened to finish in.
    found.sort_unstable();
    found
}

/// The directories under `root`, relative to it, for the pickers that ask for one.
///
/// Blocking.
#[must_use]
pub fn directories(root: &Path, options: Walk) -> Vec<String> {
    let found = Mutex::new(Vec::new());
    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .hidden(!options.hidden)
        .follow_links(options.follow_links)
        .require_git(false)
        .git_ignore(!options.ignored);

    let root = root.to_path_buf();
    builder.build_parallel().run(|| {
        let found = &found;
        let root = root.clone();
        Box::new(move |entry| {
            let Ok(entry) = entry else {
                return ignore::WalkState::Continue;
            };
            if !entry.file_type().is_some_and(|kind| kind.is_dir()) {
                return ignore::WalkState::Continue;
            }
            let path = entry.path();
            let Ok(relative) = path.strip_prefix(&root) else {
                return ignore::WalkState::Continue;
            };
            if relative.as_os_str().is_empty() {
                return ignore::WalkState::Continue;
            }
            let mut found = found
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if found.len() >= options.limit {
                return ignore::WalkState::Quit;
            }
            found.push(relative.to_string_lossy().into_owned());
            ignore::WalkState::Continue
        })
    });

    let mut found = found
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    found.sort_unstable();
    found
}

/// The absolute path a walk's answer stands for.
#[must_use]
pub fn absolute(root: &Path, relative: &str) -> PathBuf {
    root.join(relative)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small project that removes itself.
    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("zdt-walk-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("src")).expect("a directory");
            std::fs::create_dir_all(root.join("target")).expect("a directory");
            std::fs::write(root.join(".gitignore"), "target\n").expect("a file");
            std::fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("a file");
            std::fs::write(root.join("Cargo.toml"), "[package]\n").expect("a file");
            std::fs::write(root.join("target/big.bin"), "").expect("a file");
            std::fs::write(root.join(".env"), "").expect("a file");
            Self(root)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_walk_leaves_out_what_git_ignores() {
        let temp = Temp::new("ignored");
        let found = walk(&temp.0, Walk::default());
        assert!(found.contains(&"src/main.rs".to_owned()));
        assert!(found.contains(&"Cargo.toml".to_owned()));
        assert!(!found.iter().any(|path| path.starts_with("target")));
        assert!(!found.contains(&".env".to_owned()), "and dotfiles");
    }

    #[test]
    fn everything_when_it_is_asked_for() {
        let temp = Temp::new("all");
        let found = walk(
            &temp.0,
            Walk {
                ignored: true,
                hidden: true,
                ..Walk::default()
            },
        );
        assert!(found.contains(&"target/big.bin".to_owned()));
        assert!(found.contains(&".env".to_owned()));
    }

    #[test]
    fn no_directories_are_in_a_file_walk() {
        let temp = Temp::new("files");
        let found = walk(&temp.0, Walk::default());
        assert!(!found.contains(&"src".to_owned()));
    }

    #[test]
    fn the_directories_are_their_own_walk() {
        let temp = Temp::new("dirs");
        let found = directories(&temp.0, Walk::default());
        assert_eq!(found, vec!["src".to_owned()]);
    }

    #[test]
    fn a_walk_stops_at_the_limit() {
        let temp = Temp::new("limit");
        let found = walk(
            &temp.0,
            Walk {
                limit: 1,
                ..Walk::default()
            },
        );
        assert_eq!(found.len(), 1);
    }
}
