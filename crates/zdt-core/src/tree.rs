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

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::language::{self, FileType};

/// One thing in a directory.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    /// Where it is.
    pub path: PathBuf,
    /// What it is called.
    pub name: String,
    /// Whether it is a directory.
    pub directory: bool,
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

/// The tree.
#[derive(Debug)]
pub struct Tree {
    root: PathBuf,
    filter: Filter,
    /// Which directories are open.
    expanded: BTreeSet<PathBuf>,
    /// What is in each directory that has been read.
    children: BTreeMap<PathBuf, Vec<Entry>>,
}

impl Tree {
    /// A tree over `root`, with nothing open yet.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>, filter: Filter) -> Self {
        Self {
            root: root.into(),
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
    pub fn set_filter(&mut self, filter: Filter) {
        if self.filter != filter {
            self.filter = filter;
            self.children.clear();
        }
    }

    /// Whether `path` is a directory, as the rows that have been read say.
    ///
    /// Answered from what has been walked rather than from the filesystem: this is asked while a
    /// pointer moves, and a `stat` per frame is a `stat` too many.
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
        self.children
            .entry(path.to_path_buf())
            .or_insert_with(|| read(path, self.filter));
        self.expanded.insert(path.to_path_buf());
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
    /// Blocking.
    pub fn refresh(&mut self) {
        self.children.clear();
        let open: Vec<PathBuf> = self.expanded.iter().cloned().collect();
        for path in open {
            if path.is_dir() {
                self.children.insert(path.clone(), read(&path, self.filter));
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

/// What is directly inside `path`, in the order a tree shows it.
///
/// Directories first, then files, each sorted by name and ignoring case — which is the order every
/// file tree uses and the only one that can be scanned.
///
/// Blocking.
#[must_use]
pub fn read(path: &Path, filter: Filter) -> Vec<Entry> {
    let mut walk = ignore::WalkBuilder::new(path);
    walk.max_depth(Some(1))
        .hidden(!filter.hidden)
        .git_ignore(!filter.ignored)
        .git_global(!filter.ignored)
        .git_exclude(!filter.ignored)
        .ignore(!filter.ignored)
        .parents(!filter.ignored)
        // A `.gitignore` says what to leave out whether or not there is a `.git` beside it. A
        // directory somebody is editing may not be a repository yet, and the file still means
        // what it says.
        .require_git(false)
        // The tree reads one directory at a time, so there is nothing for threads to do but
        // contend.
        .follow_links(false);

    let mut entries: Vec<Entry> = walk
        .build()
        .filter_map(Result::ok)
        // The first entry a walk yields is the directory itself.
        .filter(|found| found.path() != path)
        .filter_map(|found| {
            let name = found.file_name().to_str()?.to_owned();
            Some(Entry {
                directory: found.file_type().is_some_and(|kind| kind.is_dir()),
                path: found.into_path(),
                name,
            })
        })
        .collect();

    entries.sort_by(|one, two| {
        two.directory
            .cmp(&one.directory)
            .then_with(|| one.name.to_lowercase().cmp(&two.name.to_lowercase()))
            .then_with(|| one.name.cmp(&two.name))
    });
    entries
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Filter, Tree, read};

    /// A small tree on disk: two directories, three files, one hidden, one ignored.
    fn sample() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "zdt-tree-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
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
    fn directories_come_first_and_then_names_in_order() {
        // The only order a tree can be scanned in.
        let root = sample();
        let entries = read(&root, Filter::default());
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["src", "Cargo.toml"]);
        assert!(entries[0].directory);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn what_git_ignores_is_hidden_unless_asked_for() {
        // A tree that shows `target/` is a tree nobody can find anything in.
        let root = sample();
        let plain = read(&root, Filter::default());
        assert!(!plain.iter().any(|entry| entry.name == "target"));

        let everything = read(
            &root,
            Filter {
                hidden: true,
                ignored: true,
            },
        );
        assert!(everything.iter().any(|entry| entry.name == "target"));
        assert!(everything.iter().any(|entry| entry.name == ".gitignore"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_closed_tree_has_only_what_is_open() {
        let root = sample();
        let mut tree = Tree::new(&root, Filter::default());
        assert!(tree.rows().is_empty(), "nothing has been opened");

        tree.expand(&root);
        let names: Vec<String> = tree.rows().into_iter().map(|row| row.entry.name).collect();
        assert_eq!(names, ["src", "Cargo.toml"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn opening_a_directory_puts_its_children_under_it() {
        let root = sample();
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
        let root = sample();
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
        let root = sample();
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
        let root = sample();
        let mut tree = Tree::new(&root, Filter::default());
        tree.reveal(std::path::Path::new("/etc/hosts"));
        assert!(tree.rows().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn changing_what_is_shown_reads_everything_again() {
        let root = sample();
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
        let root = sample();
        let mut tree = Tree::new(&root, Filter::default());
        tree.expand(&root);
        tree.expand(&root.join("src"));

        std::fs::remove_dir_all(root.join("src")).expect("removed");
        std::fs::write(root.join("src"), "now a file").expect("written");
        tree.refresh();

        assert!(!tree.is_expanded(&root.join("src")));
        let _ = std::fs::remove_dir_all(&root);
    }
}
