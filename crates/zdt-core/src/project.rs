//! Where the work is: the directory the editor was opened on, and what git says about it.
//!
//! A project is one root and the things derived from it. The root is what the file tree shows,
//! what the pickers search, and what a language server is told to index — so it is decided once,
//! at start-up, and everything downstream reads it rather than working it out again.

use std::path::{Path, PathBuf};

/// The markers that say a directory is the top of something.
///
/// In order: the first one found walking up wins, so a crate inside a workspace is rooted at the
/// workspace when both are there — which is what a language server wants and what a search across
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
}

impl Project {
    /// The project `path` belongs to.
    ///
    /// Walks up from `path` looking for a root marker and stops at the highest directory that has
    /// one, so a crate inside a workspace opens as the workspace. With no marker anywhere, the
    /// directory itself is the project — opening a loose file in a home directory should not make
    /// the home directory the project.
    #[must_use]
    pub fn discover(path: &Path) -> Self {
        let start = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent().unwrap_or(Path::new(".")).to_path_buf()
        };

        let mut best: Option<PathBuf> = None;
        for directory in start.ancestors() {
            if ROOT_MARKERS
                .iter()
                .any(|marker| directory.join(marker).exists())
            {
                best = Some(directory.to_path_buf());
            }
        }

        Self {
            root: best.unwrap_or(start),
        }
    }

    /// A project rooted exactly at `root`, with nothing worked out.
    #[must_use]
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The directory everything is relative to.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
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

    /// Which branch git has checked out, when the root is a repository.
    ///
    /// Read from `.git/HEAD` rather than by running git: this is drawn in the header on every
    /// frame's worth of state and must not cost a process. A detached head answers with the short
    /// commit instead, which is what it is checked out at.
    ///
    /// Blocking, but on a file of about forty bytes.
    #[must_use]
    pub fn git_branch(&self) -> Option<String> {
        let head = std::fs::read_to_string(self.root.join(".git").join("HEAD")).ok()?;
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
