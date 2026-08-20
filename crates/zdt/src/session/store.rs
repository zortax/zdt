//! Where a session is kept, and how it gets there and back.
//!
//! One directory per session, named after the directory it is for so that a person can see at a
//! glance which is which, and hashed so that two projects called `src` are two sessions.
//!
//! ```text
//! <state>/sessions/zdt-9f2c1a4b3d5e6f70/
//!     session.msgpack       the manifest
//!     buffers/0007.msgpack  one buffer's text and undo history
//! ```
//!
//! # Write order
//!
//! Blobs first, the manifest last. A crash then leaves orphan blobs — garbage, and pruned — and
//! never a manifest pointing at files that are not there. Every blob is named in the manifest
//! with its length and its hash, so one that is missing, short or wrong is skipped and that
//! buffer opens from disk with no history. One bad file never fails a whole restore.

use std::path::{Path, PathBuf};

use zdt_core::state::{self, State, StateError};

use crate::session::schema::{BufferContent, ContentRef, FORMAT, Snapshot};

/// The most sessions kept on disk.
const KEEP: usize = 100;

/// And how long one is kept after it was last written, in milliseconds.
const KEEP_MS: u64 = 90 * 24 * 60 * 60 * 1000;

/// What the session for `root` is kept in.
///
/// The last two components, sanitised, so a person can read the directory listing; and a hash of
/// the whole canonical path, which is what actually tells two of them apart.
#[must_use]
pub fn name_for(root: &Path) -> String {
    let text = root.to_string_lossy();
    let slug: String = text
        .rsplit(['/', '\\'])
        .take(2)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("-");
    let slug: String = slug
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    let slug = slug.trim_matches('_');
    let hash = state::stable_hash(text.as_bytes());
    if slug.is_empty() {
        format!("session-{hash:016x}")
    } else {
        format!("{slug}-{hash:016x}")
    }
}

/// Where the session for `root` is.
#[must_use]
pub fn directory_for(state: &State, root: &Path) -> PathBuf {
    state.sessions().join(name_for(root))
}

/// The manifest inside a session's directory.
#[must_use]
pub fn manifest_in(directory: &Path) -> PathBuf {
    directory.join("session.msgpack")
}

/// What a buffer's blob is called.
#[must_use]
pub fn blob_name(index: usize) -> String {
    format!("buffers/{index:04}.msgpack")
}

/// Reads the session for `root`, when there is one that this release can read.
///
/// Answers nothing rather than an error: a session is a convenience, and one that will not read
/// must never stop the editor opening. What went wrong is logged.
#[must_use]
pub fn read(state: &State, root: &Path) -> Option<Snapshot> {
    let path = manifest_in(&directory_for(state, root));
    let bytes = std::fs::read(&path).ok()?;
    let snapshot: Snapshot = match rmp_serde::from_slice(&bytes) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            tracing::warn!("{}: not a session ({error})", path.display());
            return None;
        }
    };
    if snapshot.format > FORMAT {
        tracing::warn!(
            "{}: written by a later zdt (format {}); ignoring it",
            path.display(),
            snapshot.format,
        );
        return None;
    }
    Some(snapshot)
}

/// Reads one buffer's blob, checking it against what the manifest said it would be.
///
/// A blob that is missing, the wrong length or the wrong hash is skipped. That buffer opens from
/// disk with no history, which is a worse session and not a broken one.
#[must_use]
pub fn read_blob(directory: &Path, reference: &ContentRef) -> Option<BufferContent> {
    let path = directory.join(&reference.file);
    let bytes = std::fs::read(&path).ok()?;
    if bytes.len() as u64 != reference.bytes || state::stable_hash(&bytes) != reference.hash {
        tracing::warn!("{}: not what the session said it was", path.display());
        return None;
    }
    match rmp_serde::from_slice::<BufferContent>(&bytes) {
        Ok(content) if content.format <= FORMAT => Some(content),
        Ok(_) => None,
        Err(error) => {
            tracing::warn!("{}: will not read ({error})", path.display());
            None
        }
    }
}

/// Writes one buffer's blob, and answers how to find it again.
///
/// # Errors
///
/// When the bytes cannot be written.
pub fn write_blob(
    directory: &Path,
    index: usize,
    content: &BufferContent,
) -> Result<ContentRef, StateError> {
    let file = blob_name(index);
    let bytes = rmp_serde::to_vec_named(content).map_err(|error| StateError::Malformed {
        path: directory.join(&file),
        reason: error.to_string(),
    })?;
    state::write_atomically(&directory.join(&file), &bytes)?;
    Ok(ContentRef {
        file,
        bytes: bytes.len() as u64,
        hash: state::stable_hash(&bytes),
    })
}

/// Writes the manifest. Call this after every blob it names.
///
/// # Errors
///
/// When the bytes cannot be written.
pub fn write_manifest(directory: &Path, snapshot: &Snapshot) -> Result<(), StateError> {
    let path = manifest_in(directory);
    let bytes = rmp_serde::to_vec_named(snapshot).map_err(|error| StateError::Malformed {
        path: path.clone(),
        reason: error.to_string(),
    })?;
    state::write_atomically(&path, &bytes)
}

/// Removes blobs in `directory` that `keeping` does not name.
///
/// What a save does after the manifest lands: the old numbered files are only reachable through
/// the manifest, and the manifest no longer names them.
pub fn sweep_blobs(directory: &Path, keeping: &[String]) {
    let Ok(entries) = std::fs::read_dir(directory.join("buffers")) else {
        return;
    };
    for entry in entries.flatten() {
        let name = format!("buffers/{}", entry.file_name().to_string_lossy());
        if !keeping.iter().any(|held| held == &name) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    state::sweep_unfinished(directory);
}

/// Takes the session for `root` away.
///
/// # Errors
///
/// When the directory is there and will not go.
pub fn delete(state: &State, root: &Path) -> Result<(), StateError> {
    let directory = directory_for(state, root);
    match std::fs::remove_dir_all(&directory) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(StateError::Io {
            path: directory,
            source,
        }),
    }
}

/// One session on disk, as a listing sees it.
#[derive(Clone, Debug)]
pub struct Kept {
    /// Which directory it is for.
    pub root: PathBuf,
    /// When it was last written, in milliseconds since the epoch.
    pub written_at_ms: u64,
    /// How many buffers were open in it.
    pub buffers: usize,
}

/// Every session written down, most recently written first.
///
/// Blocking. Called on a worker.
#[must_use]
pub fn list(state: &State) -> Vec<Kept> {
    let Ok(entries) = std::fs::read_dir(state.sessions()) else {
        return Vec::new();
    };
    let mut found: Vec<Kept> = entries
        .flatten()
        .filter_map(|entry| {
            let bytes = std::fs::read(manifest_in(&entry.path())).ok()?;
            let snapshot: Snapshot = rmp_serde::from_slice(&bytes).ok()?;
            Some(Kept {
                root: snapshot.root,
                written_at_ms: snapshot.written_at_ms,
                buffers: snapshot.buffers.len(),
            })
        })
        .collect();
    found.sort_unstable_by_key(|kept| std::cmp::Reverse(kept.written_at_ms));
    found
}

/// Removes sessions whose directory is gone, and then the oldest of what is left.
///
/// Blocking, and called once at startup on a worker. `keeping` is never pruned however old it is:
/// it is the session being opened.
pub fn prune(state: &State, keeping: &Path) {
    let Ok(entries) = std::fs::read_dir(state.sessions()) else {
        return;
    };
    let now = state::now_ms();
    let mut alive: Vec<(u64, PathBuf)> = Vec::new();

    for entry in entries.flatten() {
        let held = entry.path();
        let Some(snapshot) = std::fs::read(manifest_in(&held))
            .ok()
            .and_then(|bytes| rmp_serde::from_slice::<Snapshot>(&bytes).ok())
        else {
            // Not a session at all: a directory somebody made, or one whose manifest never
            // landed. Left alone rather than removed, because this is not its owner.
            continue;
        };
        if snapshot.root == keeping {
            continue;
        }
        let stale = !snapshot.root.is_dir() || now.saturating_sub(snapshot.written_at_ms) > KEEP_MS;
        if stale {
            let _ = std::fs::remove_dir_all(&held);
        } else {
            alive.push((snapshot.written_at_ms, held));
        }
    }

    if alive.len() > KEEP {
        alive.sort_unstable_by_key(|(when, _)| *when);
        for (_, held) in alive.iter().take(alive.len() - KEEP) {
            let _ = std::fs::remove_dir_all(held);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::schema::BufferSnapshot;

    fn temporary(name: &str) -> State {
        let root = std::env::temp_dir().join(format!(
            "zdt-store-{}-{name}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the directory is made");
        State::at(root)
    }

    fn snapshot(root: &Path, when: u64) -> Snapshot {
        Snapshot {
            format: FORMAT,
            written_at_ms: when,
            root: root.to_path_buf(),
            ..Snapshot::default()
        }
    }

    #[test]
    fn two_projects_with_the_same_name_are_two_sessions() {
        let one = name_for(Path::new("/home/someone/work/src"));
        let two = name_for(Path::new("/home/someone/other/src"));
        assert_ne!(one, two);
        // And both are readable at a glance.
        assert!(one.starts_with("work-src-"));
        assert!(two.starts_with("other-src-"));
    }

    #[test]
    fn a_name_is_the_same_answer_every_time() {
        // A session written by one release has to be found by the next.
        assert_eq!(
            name_for(Path::new("/home/someone/work")),
            name_for(Path::new("/home/someone/work")),
        );
    }

    #[test]
    fn an_awkward_name_still_makes_a_directory() {
        let name = name_for(Path::new("/tmp/what a name!/x y"));
        assert!(!name.contains(' '));
        assert!(!name.contains('!'));
        assert!(!name.contains('/'));
    }

    #[test]
    fn a_session_that_was_never_written_reads_as_nothing() {
        let state = temporary("absent");
        assert!(read(&state, Path::new("/nowhere")).is_none());
        let _ = std::fs::remove_dir_all(&state.root);
    }

    #[test]
    fn a_written_session_reads_back() {
        let state = temporary("roundtrip");
        let root = Path::new("/home/someone/work");
        let directory = directory_for(&state, root);

        let mut held = snapshot(root, 1234);
        held.buffers.push(BufferSnapshot::default());
        write_manifest(&directory, &held).expect("it writes");

        let back = read(&state, root).expect("it reads");
        assert_eq!(back.root, root);
        assert_eq!(back.written_at_ms, 1234);
        assert_eq!(back.buffers.len(), 1);
        let _ = std::fs::remove_dir_all(&state.root);
    }

    #[test]
    fn a_session_from_a_later_release_is_ignored_rather_than_guessed_at() {
        let state = temporary("future");
        let root = Path::new("/home/someone/work");
        let mut held = snapshot(root, 1);
        held.format = FORMAT + 1;
        write_manifest(&directory_for(&state, root), &held).expect("it writes");

        assert!(read(&state, root).is_none());
        let _ = std::fs::remove_dir_all(&state.root);
    }

    #[test]
    fn a_blob_that_does_not_match_the_manifest_is_skipped() {
        // Which is what a half-written or truncated file looks like.
        let state = temporary("blob");
        let directory = directory_for(&state, Path::new("/home/someone/work"));
        let content = BufferContent {
            format: FORMAT,
            text: Some("hello".to_owned()),
            ..BufferContent::default()
        };
        let mut reference = write_blob(&directory, 7, &content).expect("it writes");
        assert!(read_blob(&directory, &reference).is_some());

        reference.hash ^= 1;
        assert!(read_blob(&directory, &reference).is_none());
        let _ = std::fs::remove_dir_all(&state.root);
    }

    #[test]
    fn a_blob_the_manifest_no_longer_names_is_swept_away() {
        let state = temporary("sweep");
        let directory = directory_for(&state, Path::new("/home/someone/work"));
        let content = BufferContent::default();
        let kept = write_blob(&directory, 1, &content).expect("it writes");
        let gone = write_blob(&directory, 2, &content).expect("it writes");

        sweep_blobs(&directory, std::slice::from_ref(&kept.file));
        assert!(directory.join(&kept.file).exists());
        assert!(!directory.join(&gone.file).exists());
        let _ = std::fs::remove_dir_all(&state.root);
    }

    #[test]
    fn a_session_whose_directory_is_gone_is_pruned() {
        let state = temporary("prune");
        let gone = state.root.join("was-here");
        write_manifest(
            &directory_for(&state, &gone),
            &snapshot(&gone, state::now_ms()),
        )
        .expect("it writes");
        assert!(read(&state, &gone).is_some());

        prune(&state, Path::new("/somewhere/else"));
        assert!(read(&state, &gone).is_none());
        let _ = std::fs::remove_dir_all(&state.root);
    }

    #[test]
    fn the_session_being_opened_is_never_pruned() {
        let state = temporary("keeping");
        // A directory that is not there, which would otherwise be pruned at once.
        let opening = state.root.join("not-there");
        write_manifest(
            &directory_for(&state, &opening),
            &snapshot(&opening, state::now_ms()),
        )
        .expect("it writes");

        prune(&state, &opening);
        assert!(read(&state, &opening).is_some());
        let _ = std::fs::remove_dir_all(&state.root);
    }

    #[test]
    fn a_listing_puts_the_most_recent_first() {
        let state = temporary("list");
        for (name, when) in [("old", 10), ("new", 30), ("middle", 20)] {
            let root = state.root.join(name);
            std::fs::create_dir_all(&root).expect("the directory is made");
            write_manifest(&directory_for(&state, &root), &snapshot(&root, when))
                .expect("it writes");
        }
        let listed = list(&state);
        let names: Vec<String> = listed
            .iter()
            .map(|kept| {
                kept.root
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, vec!["new", "middle", "old"]);
        let _ = std::fs::remove_dir_all(&state.root);
    }
}
