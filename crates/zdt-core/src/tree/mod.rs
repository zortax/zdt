//! The file tree, as a flat list of rows.
//!
//! A tree drawn as a tree is a tree that cannot be virtualised, and a project with a node_modules
//! in it will have a hundred thousand rows in it the moment somebody expands the wrong directory.
//! So the shape is a tree and the drawing is a list: [`Tree::rows`] flattens what is expanded, and
//! the interface draws a window onto that.
//!
//! Directories are read when they are opened and remembered afterwards, so closing and reopening
//! one costs nothing. What `.gitignore` hides is hidden unless asked for, because a file tree that
//! shows `target/` is a file tree nobody can find anything in.
//!
//! Every entry carries what sets it apart, and [`Filter`] decides from that what is shown. Asked
//! for, a dotfile and an ignored directory come back with the rest and the interface draws them
//! faintly.

pub mod read;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::language::{self, FileType};
pub use crate::tree::read::{Ignores, read, read_with};

/// What sets an entry apart from an ordinary one.
///
/// Two independent facts. A dotfile that git also leaves out is both.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Standing {
    /// Whether its name begins with a dot.
    pub hidden: bool,
    /// Whether the ignore files leave it out.
    pub ignored: bool,
}

impl Standing {
    /// Whether anything sets it apart at all.
    #[must_use]
    pub const fn is_apart(self) -> bool {
        self.hidden || self.ignored
    }
}

/// One thing in a directory.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    /// Where it is.
    pub path: PathBuf,
    /// What it is called.
    pub name: String,
    /// Whether it is a directory.
    pub directory: bool,
    /// What sets it apart.
    pub standing: Standing,
}

impl Entry {
    /// What kind of thing it is, for its glyph and its grammar.
    #[must_use]
    pub fn file_type(&self) -> FileType {
        if self.directory {
            language::DIRECTORY
        } else {
            language::of(&self.path)
        }
    }
}

/// One row of the drawn list.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Row {
    /// What it stands for.
    pub entry: Entry,
    /// How far in it is, counting the root as zero.
    pub depth: usize,
    /// Whether it is a directory that is open.
    pub expanded: bool,
}

/// What to show and what to leave out.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Filter {
    /// Whether to show what begins with a dot.
    pub hidden: bool,
    /// Whether to show what git ignores.
    pub ignored: bool,
}

impl Filter {
    /// Whether an entry with this standing is shown.
    ///
    /// Each thing that sets an entry apart needs its own permission, so a dotfile that git also
    /// leaves out appears once both are asked for.
    #[must_use]
    pub const fn keeps(self, standing: Standing) -> bool {
        (self.hidden || !standing.hidden) && (self.ignored || !standing.ignored)
    }
}

/// The tree.
#[derive(Debug)]
pub struct Tree {
    root: PathBuf,
    filter: Filter,
    /// Which directories are open.
    expanded: BTreeSet<PathBuf>,
    /// What is in each directory that has been read.
    children: BTreeMap<PathBuf, Vec<Entry>>,
    /// The ignore files over the whole tree, read once and kept.
    ignores: Ignores,
}

impl Tree {
    /// A tree over `root`, with nothing open yet.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>, filter: Filter) -> Self {
        let root = root.into();
        Self {
            ignores: Ignores::new(&root),
            root,
            filter,
            expanded: BTreeSet::new(),
            children: BTreeMap::new(),
        }
    }

    /// The directory it is rooted at.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// What it is showing and leaving out.
    #[must_use]
    pub fn filter(&self) -> Filter {
        self.filter
    }

    /// Changes what it shows, and forgets everything read under the old rule.
    ///
    /// The ignore files are kept: what they say did not change, only what is done about it.
    pub fn set_filter(&mut self, filter: Filter) {
        if self.filter != filter {
            self.filter = filter;
            self.children.clear();
        }
    }

    /// Whether `path` is a directory, as the rows that have been read say.
    ///
    /// Answered from what has been walked, and never from the filesystem. This is asked while a
    /// pointer moves, and one `stat` per frame is one too many.
    #[must_use]
    pub fn is_directory(&self, path: &Path) -> bool {
        if path == self.root {
            return true;
        }
        let Some(parent) = path.parent() else {
            return false;
        };
        self.children.get(parent).is_some_and(|entries| {
            entries
                .iter()
                .any(|entry| entry.path == path && entry.directory)
        })
    }

    /// Whether `path` is an open directory.
    #[must_use]
    pub fn is_expanded(&self, path: &Path) -> bool {
        self.expanded.contains(path)
    }

    /// Opens `path`, reading it if it has not been read.
    ///
    /// Blocking. Called from a worker, because a directory on a network share can take as long as
    /// it likes.
    pub fn expand(&mut self, path: &Path) {
        if !path.is_dir() {
            return;
        }
        if !self.children.contains_key(path) {
            let entries = read_with(path, self.filter, &mut self.ignores);
            self.children.insert(path.to_path_buf(), entries);
        }
        self.expanded.insert(path.to_path_buf());
    }

    /// Every directory that is open, parents before children.
    ///
    /// A `BTreeSet` underneath, so the order is already the one [`expand`](Self::expand) needs:
    /// it only reads a directory it can already see.
    #[must_use]
    pub fn expanded(&self) -> Vec<PathBuf> {
        self.expanded.iter().cloned().collect()
    }

    /// Opens each of `paths`, in the order given.
    ///
    /// Blocking, and one hop rather than one per directory: expanding reads a directory, and
    /// forty of those on the interface thread is a frame nobody sees the end of. Paths that are
    /// no longer directories are skipped.
    pub fn restore_expanded(&mut self, paths: &[PathBuf]) {
        for path in paths {
            self.expand(path);
        }
    }

    /// Closes `path`. What was read stays read.
    pub fn collapse(&mut self, path: &Path) {
        self.expanded.remove(path);
    }

    /// Opens it if it is closed and closes it if it is open.
    pub fn toggle(&mut self, path: &Path) {
        if self.is_expanded(path) {
            self.collapse(path);
        } else {
            self.expand(path);
        }
    }

    /// Forgets what was read, keeping what is open, and reads it again.
    ///
    /// The ignore files are read again too, so a `.gitignore` that has just been written is the
    /// one that decides what shows.
    ///
    /// Blocking.
    pub fn refresh(&mut self) {
        self.ignores = Ignores::new(&self.root);
        self.children.clear();
        let open: Vec<PathBuf> = self.expanded.iter().cloned().collect();
        for path in open {
            if path.is_dir() {
                let entries = read_with(&path, self.filter, &mut self.ignores);
                self.children.insert(path, entries);
            } else {
                // It was a directory when it was opened and is not one now.
                self.expanded.remove(&path);
            }
        }
    }

    /// Opens every directory on the way to `path`, so it can be shown.
    ///
    /// Blocking. What "reveal the current file" is.
    pub fn reveal(&mut self, path: &Path) {
        let Ok(inside) = path.strip_prefix(&self.root) else {
            return;
        };
        let mut walk = self.root.clone();
        self.expand(&walk.clone());
        for part in inside.components() {
            walk.push(part);
            if walk == path {
                break;
            }
            self.expand(&walk.clone());
        }
    }

    /// Every row, in the order they are drawn.
    #[must_use]
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        self.collect(&self.root, 0, &mut rows);
        rows
    }

    /// The rows under `directory`, and their children when it is open.
    fn collect(&self, directory: &Path, depth: usize, into: &mut Vec<Row>) {
        let Some(entries) = self.children.get(directory) else {
            return;
        };
        for entry in entries {
            let expanded = entry.directory && self.expanded.contains(&entry.path);
            into.push(Row {
                entry: entry.clone(),
                depth,
                expanded,
            });
            if expanded {
                self.collect(&entry.path, depth + 1, into);
            }
        }
    }

    /// Where `path` is in the drawn list, when it is in it.
    #[must_use]
    pub fn index_of(&self, path: &Path) -> Option<usize> {
        self.rows().iter().position(|row| row.entry.path == path)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Filter, Standing, Tree};

    /// A small tree on disk: two directories, three files, one hidden, one ignored.
    fn sample(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("zdt-tree-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).expect("made");
        std::fs::create_dir_all(root.join("target")).expect("made");
        std::fs::write(root.join("Cargo.toml"), "").expect("written");
        std::fs::write(root.join(".gitignore"), "target\n").expect("written");
        std::fs::write(root.join("src/main.rs"), "").expect("written");
        std::fs::write(root.join("src/lib.rs"), "").expect("written");
        std::fs::write(root.join("target/debug"), "").expect("written");
        root
    }

    #[test]
    fn each_thing_that_sets_an_entry_apart_needs_its_own_permission() {
        let apart = Standing {
            hidden: true,
            ignored: true,
        };
        let hidden = Standing {
            hidden: true,
            ignored: false,
        };
        let plain = Standing::default();

        assert!(Filter::default().keeps(plain));
        assert!(!Filter::default().keeps(hidden));
        assert!(
            !Filter {
                hidden: true,
                ignored: false
            }
            .keeps(apart)
        );
        assert!(
            Filter {
                hidden: true,
                ignored: true
            }
            .keeps(apart)
        );
    }

    #[test]
    fn a_closed_tree_has_only_what_is_open() {
        let root = sample("closed");
        let mut tree = Tree::new(&root, Filter::default());
        assert!(tree.rows().is_empty(), "nothing has been opened");

        tree.expand(&root);
        let names: Vec<String> = tree.rows().into_iter().map(|row| row.entry.name).collect();
        assert_eq!(names, ["src", "Cargo.toml"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn opening_a_directory_puts_its_children_under_it() {
        let root = sample("children");
        let mut tree = Tree::new(&root, Filter::default());
        tree.expand(&root);
        tree.expand(&root.join("src"));

        let rows = tree.rows();
        let described: Vec<(String, usize)> = rows
            .iter()
            .map(|row| (row.entry.name.clone(), row.depth))
            .collect();
        assert_eq!(
            described,
            [
                ("src".to_owned(), 0),
                ("lib.rs".to_owned(), 1),
                ("main.rs".to_owned(), 1),
                ("Cargo.toml".to_owned(), 0),
            ]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn closing_a_directory_hides_its_children_and_keeps_what_was_read() {
        let root = sample("closing");
        let mut tree = Tree::new(&root, Filter::default());
        tree.expand(&root);
        tree.expand(&root.join("src"));
        tree.collapse(&root.join("src"));

        assert_eq!(tree.rows().len(), 2);
        assert!(!tree.is_expanded(&root.join("src")));
        // Reopening costs nothing, because what was read is still there.
        tree.toggle(&root.join("src"));
        assert_eq!(tree.rows().len(), 4);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn revealing_a_file_opens_the_way_to_it() {
        let root = sample("reveal");
        let mut tree = Tree::new(&root, Filter::default());
        tree.reveal(&root.join("src/main.rs"));

        assert!(tree.is_expanded(&root.join("src")));
        let at = tree
            .index_of(&root.join("src/main.rs"))
            .expect("it is in the list");
        assert_eq!(tree.rows()[at].entry.name, "main.rs");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn revealing_something_outside_the_project_does_nothing() {
        let root = sample("outside");
        let mut tree = Tree::new(&root, Filter::default());
        tree.reveal(std::path::Path::new("/etc/hosts"));
        assert!(tree.rows().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn changing_what_is_shown_reads_everything_again() {
        let root = sample("filter");
        let mut tree = Tree::new(&root, Filter::default());
        tree.expand(&root);
        assert_eq!(tree.rows().len(), 2);

        tree.set_filter(Filter {
            hidden: true,
            ignored: true,
        });
        // The rows are gone until what is open is read again, which is what refresh is for.
        tree.refresh();
        assert!(tree.rows().len() > 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_directory_that_has_become_a_file_stops_being_open() {
        let root = sample("became");
        let mut tree = Tree::new(&root, Filter::default());
        tree.expand(&root);
        tree.expand(&root.join("src"));

        std::fs::remove_dir_all(root.join("src")).expect("removed");
        std::fs::write(root.join("src"), "now a file").expect("written");
        tree.refresh();

        assert!(!tree.is_expanded(&root.join("src")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_rule_written_after_the_tree_was_read_decides_what_shows() {
        // What `tree.ignore` leans on: the rules are read again, so the row it named greys out.
        let root = sample("rewritten");
        let mut tree = Tree::new(&root, Filter::default());
        tree.expand(&root);
        assert!(tree.rows().iter().any(|row| row.entry.name == "src"));

        std::fs::write(root.join(".gitignore"), "target\nsrc\n").expect("written");
        tree.refresh();

        assert!(!tree.rows().iter().any(|row| row.entry.name == "src"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
