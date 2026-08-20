//! Reading a directory, and working out what each thing in it is.
//!
//! Two questions per entry: is it a directory, and does anything set it apart. The second is what
//! lets the tree draw a dotfile or an ignored build directory faintly instead of leaving it out.

use std::path::{Path, PathBuf};

use crate::tree::{Entry, Filter, Standing};

/// The ignore files over a tree, compiled and kept.
///
/// One per tree, rooted at the top of it. The rules it applies and the order it applies them in
/// are git's: a `.gitignore` in each directory on the way down, `.git/info/exclude`, the global
/// excludes file, and `.ignore` beside them. Rules for a directory are read once and kept, so
/// opening forty directories reads the chain above them once.
#[derive(Clone, Debug)]
pub struct Ignores {
    root: PathBuf,
    /// Built when something is first asked, so a tree nobody opens reads nothing.
    matcher: Option<ignore::IncrementalIgnore>,
    built: bool,
}

impl Ignores {
    /// The rules over `root`.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            matcher: None,
            built: false,
        }
    }

    /// What sets `path` apart, as its name and the ignore files say.
    ///
    /// Anything outside the root has no standing: the rules are written against paths under it.
    pub fn standing(&mut self, path: &Path, directory: bool) -> Standing {
        let hidden = path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|name| name.starts_with('.'));

        let inside = path.strip_prefix(&self.root).ok().map(Path::to_path_buf);
        let ignored = match (inside, self.matcher()) {
            (Some(inside), Some(matcher)) => matcher.matched(&inside, directory).is_ignore(),
            _ => false,
        };

        Standing { hidden, ignored }
    }

    /// The rules, built on the first question.
    fn matcher(&mut self) -> Option<&mut ignore::IncrementalIgnore> {
        if !self.built {
            self.built = true;
            self.matcher = build(&self.root);
        }
        self.matcher.as_mut()
    }
}

/// The rules over `root`, as the ignore crate reads them.
///
/// Hidden is left to the caller, so that a dotfile and an ignored file stay two different facts.
/// No depth is set either: a matcher with one answers "past the limit" for everything below it,
/// and how deep a directory is is the tree's business.
fn build(root: &Path) -> Option<ignore::IncrementalIgnore> {
    let mut walk = ignore::WalkBuilder::new(root);
    walk.hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .ignore(true)
        .parents(true)
        // A `.gitignore` says what to leave out whether or not there is a `.git` beside it. A
        // directory somebody is editing may not be a repository yet, and the file still means
        // what it says.
        .require_git(false)
        .follow_links(false);
    walk.build_matchers().pop()
}

/// What is directly inside `path`, in the order a tree shows it.
///
/// Directories first, then files, each sorted by name and ignoring case. Every file tree uses
/// that order, and it is the only one that can be scanned.
///
/// Blocking.
#[must_use]
pub fn read(path: &Path, filter: Filter) -> Vec<Entry> {
    let mut ignores = Ignores::new(path);
    read_with(path, filter, &mut ignores)
}

/// The same, against rules that are already built.
///
/// Rules rooted above `path` know that a directory on the way down is left out, and everything
/// under such a directory is left out with it. That is the rule git follows.
///
/// Blocking.
#[must_use]
pub fn read_with(path: &Path, filter: Filter, ignores: &mut Ignores) -> Vec<Entry> {
    let Ok(listing) = std::fs::read_dir(path) else {
        return Vec::new();
    };

    let mut entries: Vec<Entry> = listing
        .filter_map(Result::ok)
        .filter_map(|found| {
            let name = found.file_name().into_string().ok()?;
            let path = found.path();
            // Answered without following the link, so a link to a directory stays a file row and
            // a loop of them is not a tree with no bottom.
            let directory = found.file_type().is_ok_and(|kind| kind.is_dir());
            let standing = ignores.standing(&path, directory);
            filter.keeps(standing).then_some(Entry {
                path,
                name,
                directory,
                standing,
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

    use super::{Ignores, read};
    use crate::tree::Filter;

    /// Everything shown.
    const EVERYTHING: Filter = Filter {
        hidden: true,
        ignored: true,
    };

    /// A small tree on disk: two directories, three files, one hidden, one ignored, and one file
    /// that is both.
    fn sample(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("zdt-read-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).expect("made");
        std::fs::create_dir_all(root.join("target")).expect("made");
        std::fs::write(root.join("Cargo.toml"), "").expect("written");
        std::fs::write(root.join(".gitignore"), "target\n.env\n").expect("written");
        std::fs::write(root.join(".env"), "").expect("written");
        std::fs::write(root.join("src/main.rs"), "").expect("written");
        root
    }

    #[test]
    fn directories_come_first_and_then_names_in_order() {
        // The only order a tree can be scanned in.
        let root = sample("order");
        let entries = read(&root, Filter::default());
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["src", "Cargo.toml"]);
        assert!(entries[0].directory);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn what_is_set_apart_is_left_out_unless_asked_for() {
        // A tree that shows `target/` is a tree nobody can find anything in.
        let root = sample("apart");
        let plain = read(&root, Filter::default());
        assert!(!plain.iter().any(|entry| entry.name == "target"));

        let everything = read(&root, EVERYTHING);
        assert!(everything.iter().any(|entry| entry.name == "target"));
        assert!(everything.iter().any(|entry| entry.name == ".gitignore"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn every_entry_carries_what_sets_it_apart() {
        // Which is what lets the tree draw one faintly rather than leave it out.
        let root = sample("standing");
        let entries = read(&root, EVERYTHING);
        let standing = |name: &str| {
            entries
                .iter()
                .find(|entry| entry.name == name)
                .map(|entry| entry.standing)
                .expect("it is in the list")
        };

        assert!(!standing("src").is_apart());
        assert!(standing("target").ignored && !standing("target").hidden);
        assert!(standing(".gitignore").hidden && !standing(".gitignore").ignored);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn something_both_hidden_and_ignored_needs_both_permissions() {
        // `.env` is a dotfile that git also leaves out, so either rule on its own keeps it out.
        let root = sample("both");
        let named = |filter| {
            read(&root, filter)
                .into_iter()
                .any(|entry| entry.name == ".env")
        };

        assert!(!named(Filter::default()));
        assert!(!named(Filter {
            hidden: true,
            ignored: false
        }));
        assert!(!named(Filter {
            hidden: false,
            ignored: true
        }));
        assert!(named(EVERYTHING));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_ignored_directory_takes_everything_under_it() {
        // Git stops at the first ignored directory, so a rule that lets one file back in from
        // inside one says nothing. Rules built above the directory are what know this.
        let root = sample("terminal");
        std::fs::write(root.join(".gitignore"), "build\n!build/keep.txt\n").expect("written");
        std::fs::create_dir_all(root.join("build")).expect("made");
        std::fs::write(root.join("build/keep.txt"), "").expect("written");

        let mut ignores = Ignores::new(&root);
        assert!(ignores.standing(&root.join("build"), true).ignored);
        assert!(
            ignores
                .standing(&root.join("build/keep.txt"), false)
                .ignored
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_rule_that_leaves_the_directory_alone_lets_one_file_back_in() {
        // `build/*` leaves `build` itself out of it, so the walk goes in and the exception holds.
        let root = sample("negated");
        std::fs::write(root.join(".gitignore"), "build/*\n!build/keep.txt\n").expect("written");
        std::fs::create_dir_all(root.join("build")).expect("made");
        std::fs::write(root.join("build/keep.txt"), "").expect("written");
        std::fs::write(root.join("build/out.o"), "").expect("written");

        let mut ignores = Ignores::new(&root);
        assert!(!ignores.standing(&root.join("build"), true).ignored);
        assert!(
            !ignores
                .standing(&root.join("build/keep.txt"), false)
                .ignored
        );
        assert!(ignores.standing(&root.join("build/out.o"), false).ignored);
        let _ = std::fs::remove_dir_all(&root);
    }
}
