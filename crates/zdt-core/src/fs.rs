//! Reading a file in and writing it back out.
//!
//! Two things about a file are not in its text and have to survive a round trip anyway: how it is
//! encoded, and what it ends its lines with. Both are read on the way in, held beside the buffer,
//! and put back on the way out — so opening a CRLF file on Linux, editing one line and saving does
//! not rewrite every line in the file, and neither does opening a UTF-16 file from Windows.
//!
//! The buffer itself always holds UTF-8 with `\n` breaks. Everything above this module works in
//! those terms and never asks what the file on disk looks like.

use std::io;
use std::path::{Path, PathBuf};

/// How a file's bytes spell its text.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Encoding {
    /// UTF-8, with no byte-order mark. What almost everything is.
    #[default]
    Utf8,
    /// UTF-8 behind a byte-order mark, which some Windows tools write and expect back.
    Utf8Bom,
    /// UTF-16, little-endian, behind its mark.
    Utf16Le,
    /// UTF-16, big-endian, behind its mark.
    Utf16Be,
}

impl Encoding {
    /// How this is written in a status line.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "utf-8",
            Self::Utf8Bom => "utf-8-bom",
            Self::Utf16Le => "utf-16le",
            Self::Utf16Be => "utf-16be",
        }
    }
}

/// What a file breaks its lines with.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LineEnding {
    /// One byte. Everything but Windows.
    #[default]
    Lf,
    /// Two. Windows, and files that have been through it.
    Crlf,
}

impl LineEnding {
    /// The bytes it is.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::Crlf => "\r\n",
        }
    }

    /// How this is written in a status line.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Lf => "LF",
            Self::Crlf => "CRLF",
        }
    }

    /// Which one `text` uses, decided by the first break in it.
    ///
    /// The first rather than the most common: a file is one thing or the other, and a file that
    /// is genuinely mixed has already lost that argument. Saving normalises it, which is the only
    /// way an editor can leave a mixed file in a state anybody can reason about.
    #[must_use]
    pub fn detect(text: &str) -> Self {
        match text.find('\n') {
            Some(0) => Self::Lf,
            Some(at) if text.as_bytes()[at - 1] == b'\r' => Self::Crlf,
            _ => Self::Lf,
        }
    }
}

/// A file, read.
#[derive(Clone, Debug)]
pub struct LoadedFile {
    /// The text, as UTF-8 with `\n` breaks.
    pub text: String,
    /// How the file spelled it.
    pub encoding: Encoding,
    /// What it broke its lines with.
    pub line_ending: LineEnding,
    /// Whether bytes had to be replaced to make the text valid.
    ///
    /// A file that reads as damaged is one this editor must not write back without being told to:
    /// saving would make the damage permanent.
    pub lossy: bool,
}

/// What went wrong reading or writing a file.
#[derive(Debug, thiserror::Error)]
pub enum FileError {
    /// The file could not be read or written.
    #[error("{path}: {source}")]
    Io {
        /// Which file.
        path: PathBuf,
        /// What the system said.
        #[source]
        source: io::Error,
    },
    /// The file is not text this editor can show.
    #[error("{path}: not text")]
    NotText {
        /// Which file.
        path: PathBuf,
    },
}

impl FileError {
    fn io(path: &Path, source: io::Error) -> Self {
        Self::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

/// How large a file may be before this refuses to open it, in bytes.
///
/// The editor itself is happy with far more; the limit is on reading a whole file into one string
/// on a worker, which is what opening does. A file past this is one a person meant to open by
/// accident.
pub const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

/// Reads `path`, normalising its line endings and remembering what they were.
///
/// Blocking. Called from a worker.
pub fn load(path: &Path) -> Result<LoadedFile, FileError> {
    let metadata = std::fs::metadata(path).map_err(|error| FileError::io(path, error))?;
    if metadata.len() > MAX_FILE_BYTES {
        return Err(FileError::NotText {
            path: path.to_path_buf(),
        });
    }
    let bytes = std::fs::read(path).map_err(|error| FileError::io(path, error))?;
    decode(&bytes, path)
}

/// Turns a file's bytes into text, its encoding and its line ending.
pub fn decode(bytes: &[u8], path: &Path) -> Result<LoadedFile, FileError> {
    let (encoding, body) = sniff(bytes);
    let (text, lossy) = match encoding {
        Encoding::Utf8 | Encoding::Utf8Bom => match std::str::from_utf8(body) {
            Ok(text) => (text.to_owned(), false),
            Err(_) => (String::from_utf8_lossy(body).into_owned(), true),
        },
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let big = encoding == Encoding::Utf16Be;
            let units: Vec<u16> = body
                .chunks_exact(2)
                .map(|pair| {
                    if big {
                        u16::from_be_bytes([pair[0], pair[1]])
                    } else {
                        u16::from_le_bytes([pair[0], pair[1]])
                    }
                })
                .collect();
            match String::from_utf16(&units) {
                Ok(text) => (text, false),
                Err(_) => (String::from_utf16_lossy(&units), true),
            }
        }
    };

    // A file with a zero byte in it is not text, whatever it decoded to. Checked after decoding
    // rather than before, because UTF-16 is full of them by construction.
    if text.contains('\0') {
        return Err(FileError::NotText {
            path: path.to_path_buf(),
        });
    }

    let line_ending = LineEnding::detect(&text);
    let text = match line_ending {
        LineEnding::Lf => text,
        LineEnding::Crlf => text.replace("\r\n", "\n"),
    };

    Ok(LoadedFile {
        text,
        encoding,
        line_ending,
        lossy,
    })
}

/// Writes `text` to `path` in `encoding` with `line_ending`, atomically.
///
/// Through a temporary beside the target and a rename, so an interrupted save leaves the old file
/// rather than half of the new one. Beside rather than in the system's temporary directory,
/// because a rename across filesystems is a copy and stops being atomic.
///
/// Blocking. Called from a worker.
pub fn save(
    path: &Path,
    text: &str,
    encoding: Encoding,
    line_ending: LineEnding,
) -> Result<(), FileError> {
    let bytes = encode(text, encoding, line_ending);

    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(directory).map_err(|error| FileError::io(path, error))?;

    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    let temporary = directory.join(format!(
        ".{}.zdt-{}",
        name.as_deref().unwrap_or("buffer"),
        std::process::id()
    ));

    std::fs::write(&temporary, &bytes).map_err(|error| FileError::io(&temporary, error))?;

    // The permissions the file already had, so saving does not make an executable script
    // unexecutable.
    #[cfg(unix)]
    if let Ok(metadata) = std::fs::metadata(path) {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = metadata.permissions().mode();
        let _ = std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(mode));
    }

    std::fs::rename(&temporary, path).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        FileError::io(path, error)
    })
}

/// The bytes `text` is in `encoding` with `line_ending`.
#[must_use]
pub fn encode(text: &str, encoding: Encoding, line_ending: LineEnding) -> Vec<u8> {
    let text = match line_ending {
        LineEnding::Lf => std::borrow::Cow::Borrowed(text),
        LineEnding::Crlf => std::borrow::Cow::Owned(text.replace('\n', "\r\n")),
    };

    match encoding {
        Encoding::Utf8 => text.into_owned().into_bytes(),
        Encoding::Utf8Bom => {
            let mut bytes = vec![0xEF, 0xBB, 0xBF];
            bytes.extend_from_slice(text.as_bytes());
            bytes
        }
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let big = encoding == Encoding::Utf16Be;
            let mut bytes = if big {
                vec![0xFE, 0xFF]
            } else {
                vec![0xFF, 0xFE]
            };
            for unit in text.encode_utf16() {
                bytes.extend_from_slice(&if big {
                    unit.to_be_bytes()
                } else {
                    unit.to_le_bytes()
                });
            }
            bytes
        }
    }
}

/// The encoding a byte-order mark declares, and the bytes after it.
fn sniff(bytes: &[u8]) -> (Encoding, &[u8]) {
    match bytes {
        [0xEF, 0xBB, 0xBF, rest @ ..] => (Encoding::Utf8Bom, rest),
        [0xFF, 0xFE, rest @ ..] => (Encoding::Utf16Le, rest),
        [0xFE, 0xFF, rest @ ..] => (Encoding::Utf16Be, rest),
        rest => (Encoding::Utf8, rest),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Encoding, LineEnding, decode, encode};

    fn read(bytes: &[u8]) -> super::LoadedFile {
        decode(bytes, Path::new("test.txt")).expect("the bytes are text")
    }

    #[test]
    fn plain_utf8_is_the_ordinary_case() {
        let file = read(b"one\ntwo\n");
        assert_eq!(file.text, "one\ntwo\n");
        assert_eq!(file.encoding, Encoding::Utf8);
        assert_eq!(file.line_ending, LineEnding::Lf);
        assert!(!file.lossy);
    }

    #[test]
    fn windows_breaks_are_normalised_and_remembered() {
        // The buffer never sees a carriage return, and the file gets every one of them back.
        let file = read(b"one\r\ntwo\r\n");
        assert_eq!(file.text, "one\ntwo\n");
        assert_eq!(file.line_ending, LineEnding::Crlf);
        assert_eq!(
            encode(&file.text, file.encoding, file.line_ending),
            b"one\r\ntwo\r\n"
        );
    }

    #[test]
    fn a_mark_survives_the_round_trip() {
        for (bytes, encoding) in [
            (b"\xEF\xBB\xBFhi".to_vec(), Encoding::Utf8Bom),
            (b"\xFF\xFEh\0i\0".to_vec(), Encoding::Utf16Le),
            (b"\xFE\xFF\0h\0i".to_vec(), Encoding::Utf16Be),
        ] {
            let file = read(&bytes);
            assert_eq!(file.text, "hi", "{encoding:?}");
            assert_eq!(file.encoding, encoding);
            assert_eq!(
                encode(&file.text, file.encoding, file.line_ending),
                bytes,
                "{encoding:?} did not come back as it went in"
            );
        }
    }

    #[test]
    fn broken_bytes_are_shown_and_flagged() {
        // Shown, because a file nobody can open is worse than one with a replacement character in
        // it; flagged, because saving it would make the damage permanent.
        let file = read(b"good \xFF bad");
        assert!(file.lossy);
        assert!(file.text.contains('\u{FFFD}'));
    }

    #[test]
    fn something_with_a_zero_byte_in_it_is_not_text() {
        assert!(decode(b"\x7FELF\0\0\0", Path::new("a.out")).is_err());
    }

    #[test]
    fn an_empty_file_is_empty_text() {
        let file = read(b"");
        assert_eq!(file.text, "");
        assert_eq!(file.line_ending, LineEnding::Lf);
    }

    #[test]
    fn a_file_with_no_break_at_all_is_lf() {
        assert_eq!(LineEnding::detect("no break here"), LineEnding::Lf);
    }

    #[test]
    fn a_leading_break_is_lf() {
        // `text[at - 1]` would read before the start; this is the case that catches it.
        assert_eq!(LineEnding::detect("\nsecond"), LineEnding::Lf);
    }

    #[test]
    fn the_first_break_decides() {
        assert_eq!(LineEnding::detect("one\r\ntwo\nthree"), LineEnding::Crlf);
        assert_eq!(LineEnding::detect("one\ntwo\r\nthree"), LineEnding::Lf);
    }

    #[test]
    fn a_file_written_out_reads_back_the_same() {
        let directory = std::env::temp_dir().join(format!("zdt-fs-{}", std::process::id()));
        let path = directory.join("round-trip.txt");
        super::save(&path, "one\ntwo\n", Encoding::Utf8Bom, LineEnding::Crlf)
            .expect("the file writes");

        let file = super::load(&path).expect("the file reads");
        assert_eq!(file.text, "one\ntwo\n");
        assert_eq!(file.encoding, Encoding::Utf8Bom);
        assert_eq!(file.line_ending, LineEnding::Crlf);

        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn saving_leaves_nothing_beside_the_file() {
        // The temporary is renamed, not left; a directory listing after a save has one file in it.
        let directory = std::env::temp_dir().join(format!("zdt-fs-clean-{}", std::process::id()));
        let path = directory.join("only.txt");
        super::save(&path, "hi\n", Encoding::Utf8, LineEnding::Lf).expect("the file writes");
        super::save(&path, "hi again\n", Encoding::Utf8, LineEnding::Lf).expect("it writes again");

        let entries: Vec<_> = std::fs::read_dir(&directory)
            .expect("the directory is there")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(entries.len(), 1, "{entries:?}");

        let _ = std::fs::remove_dir_all(&directory);
    }
}
