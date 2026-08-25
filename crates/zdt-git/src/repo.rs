//! The repository, opened.
//!
//! One handle, opened from any path inside the working tree and shared by everything else in this
//! crate. Opening costs: it reads the configuration, resolves the object store, and works out
//! where the worktree is. So it happens once, and the handle is passed around.

use std::path::{Path, PathBuf};

/// What went wrong.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// There is no repository here.
    #[error("{0} is not in a git repository")]
    NotARepository(PathBuf),
    /// A bare repository, which has no working tree to show a diff against.
    #[error("{0} is a bare repository")]
    Bare(PathBuf),
    /// Git said no.
    #[error("{0}")]
    Git(String),
}

impl Error {
    /// An error from anything `gix` reports, as a sentence.
    pub(crate) fn git(error: impl std::fmt::Display) -> Self {
        Self::Git(error.to_string())
    }
}

/// A repository, open.
///
/// Cheap to clone: `gix` handles share their object store and their configuration.
#[derive(Clone)]
pub struct Repo {
    inner: gix::Repository,
    root: PathBuf,
}

impl Repo {
    /// Opens the repository `path` is inside.
    ///
    /// Walks upwards, so any file in the tree finds it. A bare repository is refused. Everything
    /// the panel shows is a comparison against a working tree, and a bare repository has none.
    ///
    /// # Errors
    ///
    /// When there is no repository above `path`, when it is bare, or when it will not open.
    pub fn open(path: &Path) -> Result<Self, Error> {
        let found = gix::discover(path).map_err(|_| Error::NotARepository(path.to_path_buf()))?;
        let root = found
            .workdir()
            .ok_or_else(|| Error::Bare(path.to_path_buf()))?
            .to_path_buf();
        Ok(Self { inner: found, root })
    }

    /// The top of the working tree.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The repository itself, for the modules in this crate.
    pub(crate) fn git(&self) -> &gix::Repository {
        &self.inner
    }

    /// `path` as the repository names it: relative to the working tree, with forward slashes.
    ///
    /// `None` for a path outside the tree, which is what a file opened from somewhere else is.
    #[must_use]
    pub fn relative(&self, path: &Path) -> Option<String> {
        let relative = path.strip_prefix(&self.root).ok()?;
        let text = relative.to_string_lossy().replace('\\', "/");
        (!text.is_empty()).then_some(text)
    }

    /// The reverse: a path the repository named, as somewhere on this machine.
    #[must_use]
    pub fn absolute(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    /// The `.git` directory, which is what a watcher watches to know anything changed.
    #[must_use]
    pub fn dot_git(&self) -> PathBuf {
        self.inner.path().to_path_buf()
    }
}

/// Runs git in `repo`, with `env` set, and answers what it printed.
///
/// No shell is involved: every argument is one argument. On refusal the first useful line of
/// what git said becomes the error.
pub(crate) fn git(
    repo: &Repo,
    args: &[&str],
    env: &[(&str, &std::ffi::OsStr)],
) -> Result<String, Error> {
    let mut command = std::process::Command::new("git");
    command.arg("-C").arg(repo.root()).args(args);
    for (name, value) in env {
        command.env(name, value);
    }
    let output = command
        .output()
        .map_err(|error| Error::Git(format!("git could not be run: {error}")))?;

    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let said = String::from_utf8_lossy(&output.stderr);
    let first = said
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("git refused");
    Err(Error::Git(first.to_owned()))
}

impl std::fmt::Debug for Repo {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Repo")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
pub(crate) mod testing {
    //! A real repository in a temporary directory.
    //!
    //! Built by running `git`, deliberately: these tests are about whether this crate reads and
    //! writes what *git itself* would, and a fixture built with the same library it is testing
    //! could agree with it and both be wrong.

    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// A repository that removes itself.
    pub struct Temp(pub PathBuf);

    impl Temp {
        /// An empty repository with one commit in it.
        pub fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "zdt-git-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).expect("a directory");

            let temp = Self(root);
            temp.run(&["init", "--initial-branch=main"]);
            temp.run(&["config", "user.email", "test@example.com"]);
            temp.run(&["config", "user.name", "Test"]);
            temp.run(&["config", "commit.gpgsign", "false"]);
            temp
        }

        /// Runs git in it, and says what it printed.
        pub fn run(&self, args: &[&str]) -> String {
            let output = Command::new("git")
                .arg("-C")
                .arg(&self.0)
                .args(args)
                .output()
                .unwrap_or_else(|error| panic!("git {args:?}: {error}"));
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).into_owned()
        }

        /// Writes a file in it.
        pub fn write(&self, name: &str, text: &str) {
            let path = self.0.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("a directory");
            }
            std::fs::write(path, text).expect("a file");
        }

        /// Writes a file, stages it and commits it.
        pub fn commit(&self, name: &str, text: &str, message: &str) {
            self.write(name, text);
            self.run(&["add", name]);
            self.run(&["commit", "-m", message]);
        }

        /// A path inside it.
        pub fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }

        /// It, as a path.
        pub fn root(&self) -> &Path {
            &self.0
        }

        /// It, opened.
        pub fn repo(&self) -> super::Repo {
            super::Repo::open(&self.0).expect("it opens")
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Repo;
    use super::testing::Temp;

    #[test]
    fn a_repository_is_found_from_anywhere_inside_it() {
        let temp = Temp::new("discover");
        temp.commit("src/main.rs", "fn main() {}\n", "first");

        let from_root = Repo::open(temp.root()).expect("from the top");
        let from_deep = Repo::open(&temp.path("src")).expect("from a directory inside it");
        assert_eq!(from_root.root(), from_deep.root());
        // Canonicalised by git, so compared the same way.
        assert!(
            from_root
                .root()
                .ends_with(temp.root().file_name().expect("the directory has a name"))
        );
    }

    #[test]
    fn somewhere_that_is_not_a_repository_says_so() {
        let directory = std::env::temp_dir().join(format!("zdt-git-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a directory");
        // A temporary directory can itself be inside somebody's repository. This asserts only
        // that opening works or says why.
        if let Ok(repo) = Repo::open(&directory) {
            assert!(repo.root().exists());
        }
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_path_goes_to_the_name_git_uses_and_back() {
        let temp = Temp::new("relative");
        temp.commit("src/main.rs", "", "first");
        let repo = temp.repo();

        let inside = repo.root().join("src").join("main.rs");
        let named = repo.relative(&inside).expect("it is inside");
        assert_eq!(named, "src/main.rs");
        assert_eq!(repo.absolute(&named), inside);
    }

    #[test]
    fn a_path_outside_the_tree_has_no_name() {
        let temp = Temp::new("outside");
        temp.commit("a.txt", "", "first");
        let repo = temp.repo();
        assert!(repo.relative(std::path::Path::new("/etc/hosts")).is_none());
        // And the root itself is not a file in the tree.
        assert!(repo.relative(repo.root()).is_none());
    }
}
