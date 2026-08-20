//! Walking a project.
//!
//! One parallel walk, honouring `.gitignore`, producing every path in the project relative to its
//! root. On this machine that takes a few milliseconds for a thousand files, and well under a
//! second for a hundred thousand. It blocks, so it belongs on a worker.
//!
//! The paths come back as `String`. Everything downstream wants text: the matcher, the list, and
//! the preview's header. Converting once at the edge is cheaper than converting at every use.

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
    // Sorted, so two runs over the same project give the same order. An unmatched list then
    // reads as a tree. The order the threads finish in is arbitrary.
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

/// Every directory under `root`, no deeper than `depth`, stopping at each project.
///
/// What a sessionizer indexes: a directory of projects, and sometimes a directory of owners each
/// holding projects. Depth one is "the directories in `root`"; depth two also takes their
/// children.
///
/// A project is listed and never entered. `~/Projects/thing` is somewhere to work;
/// `~/Projects/thing/src` and `~/Projects/thing/target` are not, and a list that held every
/// subdirectory of every repository would be hundreds of rows of build output. So the depth is
/// spent on the *nesting somebody arranged* — a folder of clients each holding repositories —
/// rather than on the insides of one.
///
/// A plain walk rather than an ignore-aware one, because the directories being looked at are
/// somebody's project directory and not a repository. Reading two levels of `~/Projects` is a
/// handful of `read_dir` calls, so this is cheap, but it does block: put it on a worker.
///
/// `root` itself is never in the answer. Symbolic links are not followed, so a link back up does
/// not make the walk endless.
#[must_use]
pub fn directories_within(root: &Path, depth: usize, hidden: bool) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut level = vec![root.to_path_buf()];

    for _ in 0..depth {
        let mut next = Vec::new();
        for directory in &level {
            let Ok(entries) = std::fs::read_dir(directory) else {
                continue;
            };
            for entry in entries.flatten() {
                // `file_type` on the entry does not follow the link, which is what stops a link
                // into a parent from being walked.
                if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                    continue;
                }
                let name = entry.file_name();
                if !hidden && name.to_string_lossy().starts_with('.') {
                    continue;
                }
                let path = entry.path();
                // Listed either way; entered only when it is not itself a project.
                if !crate::Project::is_root(&path) {
                    next.push(path.clone());
                }
                found.push(path);
            }
        }
        if next.is_empty() {
            break;
        }
        level = next;
    }

    found.sort_unstable();
    found
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
    fn a_bounded_walk_stops_at_the_depth_asked_for() {
        let temp = Temp::new("bounded");
        std::fs::create_dir_all(temp.0.join("src").join("deep").join("deeper"))
            .expect("the directories are made");

        let one = directories_within(&temp.0, 1, false);
        assert!(one.contains(&temp.0.join("src")));
        assert!(!one.contains(&temp.0.join("src/deep")), "one level only");

        let two = directories_within(&temp.0, 2, false);
        assert!(two.contains(&temp.0.join("src/deep")));
        assert!(
            !two.contains(&temp.0.join("src/deep/deeper")),
            "two levels only",
        );
    }

    #[test]
    fn a_bounded_walk_lists_a_project_without_going_into_it() {
        // The whole reason a sessionizer's list is usable: `~/Projects/thing` is somewhere to
        // work, and `~/Projects/thing/src` and `~/Projects/thing/target` are not.
        let temp = Temp::new("projects");
        let repo = temp.0.join("thing");
        std::fs::create_dir_all(repo.join("src")).expect("the directories are made");
        std::fs::create_dir_all(repo.join("target")).expect("the directories are made");
        std::fs::write(repo.join("Cargo.toml"), "").expect("it writes");

        let found = directories_within(&temp.0, 2, false);
        assert!(found.contains(&repo), "the project is offered");
        assert!(!found.contains(&repo.join("src")), "its insides are not");
        assert!(!found.contains(&repo.join("target")));
    }

    #[test]
    fn a_bounded_walk_still_goes_through_a_directory_that_only_groups_projects() {
        // A folder of clients, each holding repositories, is what the second level is for.
        let temp = Temp::new("grouped");
        let repo = temp.0.join("client").join("thing");
        std::fs::create_dir_all(repo.join("src")).expect("the directories are made");
        std::fs::write(repo.join(".git"), "").expect("it writes");

        let found = directories_within(&temp.0, 2, false);
        assert!(found.contains(&temp.0.join("client")));
        assert!(found.contains(&repo), "the project inside it is reached");
    }

    #[test]
    fn a_bounded_walk_leaves_out_dotted_names_unless_asked() {
        let temp = Temp::new("dotted");
        std::fs::create_dir_all(temp.0.join(".git")).expect("the directory is made");

        assert!(!directories_within(&temp.0, 1, false).contains(&temp.0.join(".git")));
        assert!(directories_within(&temp.0, 1, true).contains(&temp.0.join(".git")));
    }

    #[test]
    fn a_bounded_walk_never_answers_with_its_own_root() {
        let temp = Temp::new("selfless");
        assert!(!directories_within(&temp.0, 2, true).contains(&temp.0));
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
