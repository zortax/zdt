//! Making, moving and unmaking files.
//!
//! The tree's `a`, `d`, `r` and `p`. All of it blocking, all of it called from a worker.
//!
//! Nothing here overwrites anything. A copy or a move onto a name that exists is an error for the
//! caller to report. The tree has no undo, so a mistake here cannot be taken back.

use std::io;
use std::path::{Path, PathBuf};

/// Makes a file, and every directory above it.
///
/// A path ending in a separator makes a directory instead, which is how neo-tree's `a` makes one
/// without a second binding.
///
/// # Errors
///
/// If the path already exists, or the filesystem refuses.
pub fn create(path: &Path, directory: bool) -> io::Result<PathBuf> {
    if path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", path.display()),
        ));
    }
    if directory {
        std::fs::create_dir_all(path)?;
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::File::create(path)?;
    }
    Ok(path.to_path_buf())
}

/// Removes a file, or a directory and everything in it.
///
/// # Errors
///
/// If the path is not there, or the filesystem refuses.
pub fn remove(path: &Path) -> io::Result<()> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// Moves `from` to `to`, falling back to copy-and-delete across filesystems.
///
/// # Errors
///
/// If `to` exists, or the filesystem refuses.
pub fn rename(from: &Path, to: &Path) -> io::Result<()> {
    if to.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", to.display()),
        ));
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        // `EXDEV`: the two are on different filesystems, and rename(2) cannot cross one.
        Err(error) if error.raw_os_error() == Some(18) => {
            copy(from, to)?;
            remove(from)
        }
        Err(error) => Err(error),
    }
}

/// Copies a file, or a whole directory.
///
/// # Errors
///
/// If `to` exists, or the filesystem refuses.
pub fn copy(from: &Path, to: &Path) -> io::Result<()> {
    if to.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", to.display()),
        ));
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if from.is_dir() {
        copy_directory(from, to)
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
}

/// A directory and everything under it, one level of recursion at a time.
fn copy_directory(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// A name that is not taken, by adding ` copy`, ` copy 2` and so on before the extension.
///
/// What a paste into the directory a file is already in does, instead of refusing.
#[must_use]
pub fn free_name(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let parent = path.parent().unwrap_or(Path::new("."));
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = path
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()));

    for attempt in 1..1000 {
        let suffix = if attempt == 1 {
            " copy".to_owned()
        } else {
            format!(" copy {attempt}")
        };
        let candidate = parent.join(format!(
            "{stem}{suffix}{}",
            extension.as_deref().unwrap_or("")
        ));
        if !candidate.exists() {
            return candidate;
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory that removes itself.
    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("zdt-paths-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("a temporary directory");
            Self(path)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn creating_makes_the_directories_above() {
        let temp = Temp::new("create");
        let path = temp.0.join("one/two/three.txt");
        create(&path, false).expect("a file");
        assert!(path.is_file());
    }

    #[test]
    fn creating_over_something_is_refused() {
        let temp = Temp::new("clash");
        let path = temp.0.join("held.txt");
        create(&path, false).expect("a file");
        assert!(create(&path, false).is_err());
    }

    #[test]
    fn a_directory_copies_with_everything_in_it() {
        let temp = Temp::new("copy");
        create(&temp.0.join("from/deep/leaf.txt"), false).expect("a file");
        copy(&temp.0.join("from"), &temp.0.join("to")).expect("a copy");
        assert!(temp.0.join("to/deep/leaf.txt").is_file());
        assert!(temp.0.join("from/deep/leaf.txt").is_file());
    }

    #[test]
    fn renaming_moves_it() {
        let temp = Temp::new("rename");
        let from = temp.0.join("before.txt");
        let to = temp.0.join("after.txt");
        create(&from, false).expect("a file");
        rename(&from, &to).expect("a rename");
        assert!(!from.exists());
        assert!(to.is_file());
    }

    #[test]
    fn a_free_name_steps_around_what_is_there() {
        let temp = Temp::new("free");
        let path = temp.0.join("note.txt");
        create(&path, false).expect("a file");
        let free = free_name(&path);
        assert_eq!(free.file_name().unwrap(), "note copy.txt");

        create(&free, false).expect("a second file");
        assert_eq!(free_name(&path).file_name().unwrap(), "note copy 2.txt");
    }

    #[test]
    fn removing_takes_a_directory_with_it() {
        let temp = Temp::new("remove");
        create(&temp.0.join("gone/leaf.txt"), false).expect("a file");
        remove(&temp.0.join("gone")).expect("a removal");
        assert!(!temp.0.join("gone").exists());
    }
}
