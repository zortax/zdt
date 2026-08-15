//! Putting the editor back the way it was.
//!
//! What is saved: the files that were open, the order they were in, which one was showing, and
//! where the caret was in each. What is not: the undo history, the folds, the terminals.
//!
//! Deliberately little. A session is a convenience for reopening yesterday's work, and every
//! additional thing in it is another thing that can be stale, wrong or enormous — a session file
//! that carries undo histories is a session file nobody can read and one bad restore away from
//! putting text back that was deliberately taken out.
//!
//! Sessions live in the configuration directory, named after the directory they were taken in, so
//! that "the session for this project" needs nothing remembered.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One saved session.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Session {
    /// Where it was taken.
    pub root: PathBuf,
    /// The files that were open, in buffer-line order.
    pub files: Vec<Entry>,
    /// Which of them was showing, by its place in `files`.
    pub showing: usize,
}

/// One file in a session.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Entry {
    /// Where it is, relative to the root when it is under it.
    pub path: PathBuf,
    /// Which line the caret was on, counting from one.
    pub line: u64,
}

impl Session {
    /// The absolute path of `entry`, whichever way it was written.
    #[must_use]
    pub fn absolute(&self, entry: &Entry) -> PathBuf {
        if entry.path.is_absolute() {
            entry.path.clone()
        } else {
            self.root.join(&entry.path)
        }
    }
}

/// What went wrong.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// There is nowhere to keep sessions.
    #[error("there is no configuration directory to keep sessions in")]
    Nowhere,
    /// The file could not be read or written.
    #[error("{path}: {source}")]
    Io {
        /// Which file.
        path: PathBuf,
        /// What the system said.
        #[source]
        source: std::io::Error,
    },
    /// The file is not a session.
    #[error("{path}: {source}")]
    Malformed {
        /// Which file.
        path: PathBuf,
        /// What was wrong with it.
        #[source]
        source: toml::de::Error,
    },
    /// There is no session for that directory.
    #[error("no session for {0}")]
    Missing(PathBuf),
}

/// Where sessions are kept.
#[must_use]
pub fn directory(paths: &zdt_core::Paths) -> PathBuf {
    paths.root.join("sessions")
}

/// What the session for `root` is called.
///
/// The path with its separators replaced, so that two projects with the same last component do not
/// share a session and a person can see at a glance which file is which.
#[must_use]
pub fn name_for(root: &Path) -> String {
    let text = root.to_string_lossy();
    let flattened: String = text
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' => '%',
            other => other,
        })
        .collect();
    format!("{}.toml", flattened.trim_start_matches('%'))
}

/// Writes `session`.
///
/// # Errors
///
/// If there is nowhere to write, or writing fails.
pub fn save(paths: &zdt_core::Paths, session: &Session) -> Result<PathBuf, SessionError> {
    let directory = directory(paths);
    std::fs::create_dir_all(&directory).map_err(|source| SessionError::Io {
        path: directory.clone(),
        source,
    })?;

    let path = directory.join(name_for(&session.root));
    let text = toml::to_string_pretty(session).unwrap_or_default();
    std::fs::write(&path, text).map_err(|source| SessionError::Io {
        path: path.clone(),
        source,
    })?;
    Ok(path)
}

/// Reads the session for `root`.
///
/// # Errors
///
/// If there is none, or it will not read.
pub fn load(paths: &zdt_core::Paths, root: &Path) -> Result<Session, SessionError> {
    let path = directory(paths).join(name_for(root));
    read(&path)
}

/// Reads one session file.
///
/// # Errors
///
/// If it is not there, or is not a session.
pub fn read(path: &Path) -> Result<Session, SessionError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(SessionError::Missing(path.to_path_buf()));
        }
        Err(source) => {
            return Err(SessionError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    toml::from_str(&text).map_err(|source| SessionError::Malformed {
        path: path.to_path_buf(),
        source,
    })
}

/// Removes the session for `root`.
///
/// # Errors
///
/// If it is there and will not go.
pub fn delete(paths: &zdt_core::Paths, root: &Path) -> Result<(), SessionError> {
    let path = directory(paths).join(name_for(root));
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(SessionError::Missing(path))
        }
        Err(source) => Err(SessionError::Io { path, source }),
    }
}

/// The session written most recently, whichever project it was for.
///
/// What `<Leader>Sl` opens: "the thing I was doing", without having to be in the directory it was
/// being done in.
#[must_use]
pub fn most_recent(paths: &zdt_core::Paths) -> Option<Session> {
    let entries = std::fs::read_dir(directory(paths)).ok()?;
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "toml") {
            continue;
        }
        let Ok(when) = entry.metadata().and_then(|data| data.modified()) else {
            continue;
        };
        if newest.as_ref().is_none_or(|(held, _)| when > *held) {
            newest = Some((when, path));
        }
    }

    read(&newest?.1).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A configuration directory that removes itself.
    struct Temp(zdt_core::Paths);

    impl Temp {
        fn new(name: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("zdt-session-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).expect("a directory");
            Self(zdt_core::Paths::at(root))
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0.root);
        }
    }

    fn session(root: &str) -> Session {
        Session {
            root: PathBuf::from(root),
            files: vec![
                Entry {
                    path: PathBuf::from("src/main.rs"),
                    line: 12,
                },
                Entry {
                    path: PathBuf::from("Cargo.toml"),
                    line: 1,
                },
            ],
            showing: 1,
        }
    }

    #[test]
    fn a_session_survives_being_written_and_read() {
        let temp = Temp::new("roundtrip");
        let saved = session("/project");
        save(&temp.0, &saved).expect("it writes");

        let read = load(&temp.0, Path::new("/project")).expect("it reads");
        assert_eq!(read, saved);
    }

    #[test]
    fn two_projects_with_the_same_name_are_two_sessions() {
        assert_ne!(
            name_for(Path::new("/one/thing")),
            name_for(Path::new("/two/thing"))
        );
        assert_eq!(name_for(Path::new("/one/thing")), "one%thing.toml");
    }

    #[test]
    fn asking_for_one_that_is_not_there_says_so() {
        let temp = Temp::new("missing");
        assert!(matches!(
            load(&temp.0, Path::new("/nowhere")),
            Err(SessionError::Missing(_))
        ));
    }

    #[test]
    fn a_file_that_is_not_a_session_says_so_rather_than_being_ignored() {
        let temp = Temp::new("malformed");
        let directory = directory(&temp.0);
        std::fs::create_dir_all(&directory).expect("a directory");
        std::fs::write(
            directory.join(name_for(Path::new("/project"))),
            "not toml {",
        )
        .expect("a file");

        assert!(matches!(
            load(&temp.0, Path::new("/project")),
            Err(SessionError::Malformed { .. })
        ));
    }

    #[test]
    fn deleting_one_that_is_not_there_says_so() {
        let temp = Temp::new("delete");
        assert!(matches!(
            delete(&temp.0, Path::new("/nowhere")),
            Err(SessionError::Missing(_))
        ));

        save(&temp.0, &session("/project")).expect("it writes");
        delete(&temp.0, Path::new("/project")).expect("it goes");
        assert!(load(&temp.0, Path::new("/project")).is_err());
    }

    #[test]
    fn the_most_recent_is_the_one_written_last() {
        let temp = Temp::new("recent");
        save(&temp.0, &session("/first")).expect("it writes");
        // The filesystem's timestamps can be coarse, so the second write is made unambiguously
        // later rather than trusting that two writes in a row differ.
        std::thread::sleep(std::time::Duration::from_millis(20));
        save(&temp.0, &session("/second")).expect("it writes");

        let found = most_recent(&temp.0).expect("there is one");
        assert_eq!(found.root, PathBuf::from("/second"));
    }

    #[test]
    fn no_sessions_at_all_is_nothing_rather_than_an_error() {
        let temp = Temp::new("none");
        assert!(most_recent(&temp.0).is_none());
    }

    #[test]
    fn a_relative_entry_is_read_against_the_root() {
        let saved = session("/project");
        assert_eq!(
            saved.absolute(&saved.files[0]),
            PathBuf::from("/project/src/main.rs")
        );

        let absolute = Entry {
            path: PathBuf::from("/elsewhere/note.md"),
            line: 1,
        };
        assert_eq!(
            saved.absolute(&absolute),
            PathBuf::from("/elsewhere/note.md"),
            "and one from outside the project is left alone"
        );
    }
}
