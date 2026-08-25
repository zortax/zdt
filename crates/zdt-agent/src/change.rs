//! What a turn changed, as the timeline carries it.
//!
//! Each settled turn that touched files gets one timeline row of kind
//! [`crate::thread::ItemKind::Diff`]. Its detail field holds a [`TurnDiff`] as JSON: which
//! checkpoints bracket the turn, and the per-file counts. The full line-by-line diff is never on
//! the wire — the editor reads it from the repository's own checkpoint refs.

use serde::{Deserialize, Serialize};

/// One turn's changes: the checkpoints around it, and the files between them.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TurnDiff {
    /// The daemon's id for the turn, which is what a revert names.
    pub turn: i64,
    /// The checkpoint ref captured before the turn ran.
    pub before: String,
    /// The checkpoint ref captured when it settled.
    pub after: String,
    /// What changed, in path order.
    pub files: Vec<FileStat>,
}

/// One changed file, by counts alone.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileStat {
    /// The file, as git names it: relative to the tree, forward slashes.
    pub path: String,
    /// Lines added.
    pub added: u32,
    /// Lines taken away.
    pub removed: u32,
    /// Whether the file is not text. A binary file has no counts worth reading.
    pub binary: bool,
}

impl TurnDiff {
    /// The whole set as the detail field's text.
    #[must_use]
    pub fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// The set a detail field holds, when it holds one.
    #[must_use]
    pub fn decode(text: &str) -> Option<Self> {
        serde_json::from_str(text).ok()
    }

    /// Lines added and taken away, across every file.
    #[must_use]
    pub fn counts(&self) -> (u32, u32) {
        self.files.iter().fold((0, 0), |(added, removed), file| {
            (added + file.added, removed + file.removed)
        })
    }

    /// The one line the row shows: "3 files  +12 −4".
    #[must_use]
    pub fn summary(&self) -> String {
        let files = self.files.len();
        let word = if files == 1 { "file" } else { "files" };
        let (added, removed) = self.counts();
        format!("{files} {word}  +{added} \u{2212}{removed}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_diff_round_trips_through_its_detail_text() {
        let held = TurnDiff {
            turn: 7,
            before: "refs/zdt/checkpoints/1/7/before".to_owned(),
            after: "refs/zdt/checkpoints/1/7/after".to_owned(),
            files: vec![FileStat {
                path: "src/main.rs".to_owned(),
                added: 12,
                removed: 4,
                binary: false,
            }],
        };
        assert_eq!(TurnDiff::decode(&held.encode()), Some(held));
    }

    #[test]
    fn a_detail_that_is_not_a_diff_reads_as_nothing() {
        assert!(TurnDiff::decode("tool output").is_none());
        assert!(TurnDiff::decode("").is_none());
    }

    #[test]
    fn the_summary_counts_every_file() {
        let held = TurnDiff {
            files: vec![
                FileStat {
                    path: "a".to_owned(),
                    added: 2,
                    removed: 1,
                    ..FileStat::default()
                },
                FileStat {
                    path: "b".to_owned(),
                    added: 3,
                    removed: 0,
                    ..FileStat::default()
                },
            ],
            ..TurnDiff::default()
        };
        assert_eq!(held.summary(), "2 files  +5 \u{2212}1");
    }
}
