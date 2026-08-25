//! A log of everything a harness said, exactly as it said it.
//!
//! One NDJSON file per adapter. Protocols drift with harness releases, and the way to diagnose
//! drift is to read what actually crossed the pipe. Writes are best-effort: a log that cannot be
//! written must never cost a turn.

use std::io::Write;
use std::path::Path;

/// One append-only NDJSON file.
pub struct RawLog {
    file: Option<std::fs::File>,
}

impl RawLog {
    /// Opens `path` for appending, making the directory above it.
    ///
    /// A path that will not open answers a log that swallows everything, and says so once.
    #[must_use]
    pub fn open(path: &Path) -> Self {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path);
        match file {
            Ok(file) => Self { file: Some(file) },
            Err(error) => {
                tracing::warn!("cannot open {}: {error}", path.display());
                Self { file: None }
            }
        }
    }

    /// A log that swallows everything.
    #[must_use]
    pub fn nowhere() -> Self {
        Self { file: None }
    }

    /// Appends one value as one line.
    pub fn line(&mut self, value: &serde_json::Value) {
        let Some(file) = self.file.as_mut() else {
            return;
        };
        let _ = serde_json::to_writer(&mut *file, value);
        let _ = file.write_all(b"\n");
    }
}
