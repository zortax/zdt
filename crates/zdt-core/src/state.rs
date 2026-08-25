//! Where the editor keeps what it worked out for itself.
//!
//! Separate from the configuration on purpose. Configuration is what a person writes; state is
//! what the editor writes about them — which files they had open, where the caret was, what the
//! splits looked like. Mixing the two would mean a directory somebody edits by hand that the
//! editor rewrites underneath them.
//!
//! It is also separate because [`crate::config`]'s directory is watched *recursively*: a session
//! written there would be a "configuration reloaded" toast every few seconds.
//!
//! ```text
//! ~/.local/state/zdt/
//!     sessions/<slug>-<hash>/
//!         session.msgpack      the manifest
//!         buffers/0007.msgpack one buffer's text and undo history
//! ```

use std::path::{Path, PathBuf};

/// Where state lives.
///
/// `$ZDT_STATE_DIR` first, so a test and a second installation both have one of their own. Then
/// the platform's state directory, which only Linux answers, and the local data directory
/// everywhere else.
#[must_use]
pub fn directory() -> Option<PathBuf> {
    if let Some(named) = std::env::var_os("ZDT_STATE_DIR") {
        return Some(PathBuf::from(named));
    }
    dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .map(|base| base.join("zdt"))
}

/// The paths inside one state directory.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct State {
    /// The directory itself.
    pub root: PathBuf,
}

impl State {
    /// The paths under `root`.
    #[must_use]
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The paths under the platform's state directory, when there is one.
    #[must_use]
    pub fn discover() -> Option<Self> {
        directory().map(Self::at)
    }

    /// Where the sessions are.
    #[must_use]
    pub fn sessions(&self) -> PathBuf {
        self.root.join("sessions")
    }

    /// Where the agent daemon keeps its database and its logs.
    #[must_use]
    pub fn agent(&self) -> PathBuf {
        self.root.join("agent")
    }
}

/// What went wrong writing state.
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    /// The file could not be written or read.
    #[error("{path}: {source}")]
    Io {
        /// Which file.
        path: PathBuf,
        /// What the system said.
        #[source]
        source: std::io::Error,
    },
    /// There is nowhere to keep state.
    #[error("there is no state directory")]
    Nowhere,
    /// The bytes are not what was expected.
    #[error("{path}: {reason}")]
    Malformed {
        /// Which file.
        path: PathBuf,
        /// What was wrong with it.
        reason: String,
    },
}

/// The extension a half-written file carries.
const WRITING: &str = "writing";

/// Writes `bytes` to `path` without ever leaving a half-written file there.
///
/// A temporary beside it, flushed, then a rename, then the directory flushed. The rename is the
/// commit point and is atomic; the two flushes are what make a machine that loses power mid-save
/// leave either the old file or the new one, and never half of either.
///
/// The temporary carries the process id, so two editors writing at once never fight over it.
///
/// # Errors
///
/// When the directory cannot be made, or either write fails.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), StateError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| StateError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let temporary = path.with_extension(format!("{}.{WRITING}", std::process::id()));
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |source| StateError::Io {
            path: path.clone(),
            source,
        }
    };

    {
        use std::io::Write;
        let mut file = std::fs::File::create(&temporary).map_err(io(&temporary))?;
        file.write_all(bytes).map_err(io(&temporary))?;
        file.sync_all().map_err(io(&temporary))?;
    }
    std::fs::rename(&temporary, path).map_err(io(path))?;

    // The rename is only durable once the directory holding it is. A crash between the two
    // leaves the old file, which is the outcome this is all for.
    if let Some(parent) = path.parent()
        && let Ok(directory) = std::fs::File::open(parent)
    {
        let _ = directory.sync_all();
    }
    Ok(())
}

/// Removes any `*.writing` files under `directory` left by a crash.
///
/// Only ones older than an hour, because another editor may be part-way through one right now.
pub fn sweep_unfinished(directory: &Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|held| held != WRITING) {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|data| data.modified())
            .is_ok_and(|when| {
                when.elapsed()
                    .is_ok_and(|since| since > std::time::Duration::from_secs(3600))
            });
        if old {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// A hash of `bytes` that means the same thing in every version of this editor.
///
/// FNV-1a, written out here rather than taken from a crate, because a file written by one release
/// has to be readable by the next and a dependency is free to change its algorithm.
#[must_use]
pub fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Milliseconds since the epoch, or zero when the clock says something impossible.
#[must_use]
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::{State, stable_hash, sweep_unfinished, write_atomically};

    fn temporary(name: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "zdt-state-{}-{name}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("the directory is made");
        directory
    }

    #[test]
    fn the_sessions_live_under_the_root() {
        let state = State::at("/somewhere");
        assert_eq!(
            state.sessions(),
            std::path::Path::new("/somewhere/sessions")
        );
    }

    #[test]
    fn the_directory_can_be_told_where_to_be() {
        // Every test needs one of its own, and so does a second installation.
        let before = std::env::var_os("ZDT_STATE_DIR");
        unsafe { std::env::set_var("ZDT_STATE_DIR", "/tmp/zdt-state-test") };
        assert_eq!(
            super::directory(),
            Some(std::path::PathBuf::from("/tmp/zdt-state-test"))
        );
        match before {
            Some(held) => unsafe { std::env::set_var("ZDT_STATE_DIR", held) },
            None => unsafe { std::env::remove_var("ZDT_STATE_DIR") },
        }
    }

    #[test]
    fn a_write_makes_the_directory_it_needs() {
        let directory = temporary("deep");
        let path = directory.join("one").join("two").join("file.msgpack");
        write_atomically(&path, b"hello").expect("it writes");
        assert_eq!(std::fs::read(&path).expect("it reads"), b"hello");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_write_leaves_nothing_half_written_behind() {
        let directory = temporary("clean");
        let path = directory.join("file.msgpack");
        write_atomically(&path, b"one").expect("it writes");
        write_atomically(&path, b"two").expect("it writes again");

        assert_eq!(std::fs::read(&path).expect("it reads"), b"two");
        let leftovers: Vec<_> = std::fs::read_dir(&directory)
            .expect("it reads")
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().contains("writing"))
            .collect();
        assert!(leftovers.is_empty(), "the temporary was renamed away");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_recent_half_written_file_is_left_alone() {
        // Another editor may be part-way through it right now.
        let directory = temporary("sweep");
        let stale = directory.join("session.msgpack.999.writing");
        std::fs::write(&stale, b"half").expect("it writes");
        sweep_unfinished(&directory);
        assert!(stale.exists());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_hash_is_the_same_answer_every_time() {
        // A file written by one release has to be readable by the next.
        assert_eq!(stable_hash(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(stable_hash(b"a"), stable_hash(b"a"));
        assert_ne!(stable_hash(b"a"), stable_hash(b"b"));
    }
}
