//! Where the work is: the directory the editor was opened on, and what git says about it.
//!
//! A project is two roots and the things derived from them. The root is the directory that was
//! opened: what the file tree shows and what the pickers search. The tooling root is whatever
//! encloses it: what a language server is told to index and where the repository is.
//!
//! The two are the same for a project opened at its own top, and differ when a subdirectory is
//! opened on its own. A server rooted at a crate inside a workspace indexes the wrong thing, and
//! a tree showing the workspace is not the subdirectory somebody asked for.

use std::path::{Path, PathBuf};

/// The markers that say a directory is the top of something.
///
/// In order. The first one found walking up wins, so a crate inside a workspace is rooted at the
/// workspace when both are there. That is what a language server wants, and what a search across
/// "the project" means.
const ROOT_MARKERS: &[&str] = &[
    ".git",
    ".jj",
    ".hg",
    "Cargo.toml",
    "package.json",
    "pyproject.toml",
    "go.mod",
];

/// The directory the editor is working in.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Project {
    root: PathBuf,
    tooling: PathBuf,
}

impl Project {
    /// Whether `path` looks like the top of a project.
    ///
    /// What a walk looking *for* projects stops at: the children of a repository are its source
    /// and its build output, and neither is somewhere to open a session.
    #[must_use]
    pub fn is_root(path: &Path) -> bool {
        ROOT_MARKERS.iter().any(|marker| path.join(marker).exists())
    }

    /// The highest directory at or above `start` that holds a root marker.
    ///
    /// The highest, so a crate inside a workspace answers with the workspace. That is what a
    /// language server wants and what a search across "the project" means. Nothing when no
    /// ancestor has a marker.
    fn enclosing(start: &Path) -> Option<PathBuf> {
        let mut best: Option<PathBuf> = None;
        for directory in start.ancestors() {
            if ROOT_MARKERS
                .iter()
                .any(|marker| directory.join(marker).exists())
            {
                best = Some(directory.to_path_buf());
            }
        }
        best
    }

    /// The directory `path` sits in, or `path` itself when it is one.
    fn directory_of(path: &Path) -> PathBuf {
        if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent().unwrap_or(Path::new(".")).to_path_buf()
        }
    }

    /// The project `path` belongs to.
    ///
    /// Both roots are the enclosing project. With no marker anywhere, the directory itself is the
    /// project. Opening a loose file in a home directory must leave the home directory alone.
    #[must_use]
    pub fn discover(path: &Path) -> Self {
        let start = Self::directory_of(path);
        let root = Self::enclosing(&start).unwrap_or(start);
        Self {
            tooling: root.clone(),
            root,
        }
    }

    /// The project for a session opened on `dir`.
    ///
    /// The root is `dir` exactly, because that is the directory somebody asked for. The tooling
    /// root is whatever encloses it, so a subdirectory of a workspace still reaches the
    /// workspace's servers and its repository.
    #[must_use]
    pub fn session(dir: &Path) -> Self {
        let root = Self::directory_of(dir);
        Self {
            tooling: Self::enclosing(&root).unwrap_or_else(|| root.clone()),
            root,
        }
    }

    /// A project rooted exactly at `root`, with nothing worked out.
    #[must_use]
    pub fn at(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            tooling: root.clone(),
            root,
        }
    }

    /// The directory everything is relative to.
    ///
    /// What the tree shows, what the pickers search, and what a path is written relative to.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The directory the servers and the repository are rooted at.
    ///
    /// At or above [`root`](Self::root). Everything that reads the filesystem *around* the work
    /// rather than *inside* it asks for this one.
    #[must_use]
    pub fn tooling_root(&self) -> &Path {
        &self.tooling
    }

    /// What the root is called, for a title bar.
    #[must_use]
    pub fn name(&self) -> String {
        self.root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.root.to_string_lossy().into_owned())
    }

    /// `path` written relative to the root, or in full when it is outside.
    ///
    /// What a buffer tab and a picker row show: a path inside the project is only interesting from
    /// the root down, and one outside it is only unambiguous in full.
    #[must_use]
    pub fn relative<'a>(&self, path: &'a Path) -> std::borrow::Cow<'a, str> {
        match path.strip_prefix(&self.root) {
            Ok(rest) => rest.to_string_lossy(),
            Err(_) => path.to_string_lossy(),
        }
    }

    /// Which branch git has checked out, when the tooling root is a repository.
    ///
    /// The tooling root, because a repository encloses the directory somebody opened as often as
    /// it is that directory.
    ///
    /// Read from `.git/HEAD`, because the header draws this on every frame's worth of state and
    /// must not cost a process. A detached head answers with the short commit it is checked out
    /// at.
    ///
    /// Blocking, but on a file of about forty bytes.
    #[must_use]
    pub fn git_branch(&self) -> Option<String> {
        let head = std::fs::read_to_string(self.tooling.join(".git").join("HEAD")).ok()?;
        let head = head.trim();
        match head.strip_prefix("ref: refs/heads/") {
            Some(branch) => Some(branch.to_owned()),
            // A detached head holds the commit itself.
            None if head.len() >= 7 => Some(head[..7].to_owned()),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::Project;

    #[test]
    fn a_path_inside_the_project_is_shown_from_the_root() {
        let project = Project::at("/home/someone/work");
        assert_eq!(
            project.relative(Path::new("/home/someone/work/src/main.rs")),
            "src/main.rs"
        );
    }

    #[test]
    fn a_path_outside_the_project_is_shown_in_full() {
        // Half a path to somewhere else says nothing about where it is.
        let project = Project::at("/home/someone/work");
        assert_eq!(
            project.relative(Path::new("/etc/hosts")),
            "/etc/hosts".to_string()
        );
    }

    #[test]
    fn the_name_is_the_last_component() {
        assert_eq!(Project::at("/home/someone/work").name(), "work");
    }

    #[test]
    fn discovery_climbs_to_the_outermost_marker() {
        // This repository is a workspace with a crate inside it; opening the crate opens the
        // workspace, which is what a search across "the project" has to mean.
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        let project = Project::discover(here);
        assert!(
            project.root().join("Cargo.toml").exists(),
            "{:?} is not a project root",
            project.root()
        );
        assert!(
            here.starts_with(project.root()),
            "the crate is inside its project"
        );
    }

    #[test]
    fn a_session_keeps_the_directory_it_was_opened_on() {
        // This crate sits inside the workspace. A session opened on it shows the crate and asks
        // the servers about the workspace, which is the whole reason there are two roots.
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        let project = Project::session(here);
        assert_eq!(project.root(), here);
        assert!(here.starts_with(project.tooling_root()));
        assert_ne!(project.root(), project.tooling_root());
    }

    #[test]
    fn a_discovered_project_has_one_root_under_both_names() {
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        let project = Project::discover(here);
        assert_eq!(project.root(), project.tooling_root());
    }

    #[test]
    fn a_session_with_nothing_above_it_is_its_own_tooling_root() {
        let directory = std::env::temp_dir().join(format!("zdt-session-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("the directory is made");
        let project = Project::session(&directory);
        assert_eq!(project.root(), directory);
        assert_eq!(project.tooling_root(), directory);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_directory_with_no_marker_above_it_is_its_own_project() {
        let directory = std::env::temp_dir().join(format!("zdt-rootless-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("the directory is made");
        let project = Project::discover(&directory);
        // The temporary directory itself, not whatever repository the system happens to have at
        // the top of the tree.
        assert_eq!(project.root(), directory);
        let _ = std::fs::remove_dir_all(&directory);
    }
}
