//! The diff, flattened into the rows a virtual list draws.

use zdt_git::FileDiff;

/// One row of the diff, as it is drawn.
///
/// The diff arrives as files holding hunks holding lines, which is the right shape to *think*
/// about and the wrong one to draw: a virtual list has to know how many rows there are without
/// building any of them, and a nested structure cannot say. So it is flattened once, here.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum DiffRow {
    /// A file's heading, and how much it changed.
    File {
        /// Which file.
        path: String,
        /// How many lines it adds.
        added: usize,
        /// How many it takes away.
        removed: usize,
        /// Whether there is nothing to show because it is not text.
        binary: bool,
    },
    /// A hunk's `@@` line.
    Hunk {
        /// What it says.
        header: String,
        /// Which hunk of the whole diff this is.
        hunk: usize,
    },
    /// One line of one hunk.
    Line {
        /// What happened to it.
        kind: zdt_git::LineKind,
        /// The text.
        text: String,
        /// Its number in the old file, when it has one.
        old: Option<u32>,
        /// Its number in the new file.
        new: Option<u32>,
        /// Which hunk it belongs to.
        hunk: usize,
    },
}

impl DiffRow {
    /// Which hunk this row belongs to, when it belongs to one.
    #[must_use]
    pub const fn hunk(&self) -> Option<usize> {
        match self {
            Self::File { .. } => None,
            Self::Hunk { hunk, .. } | Self::Line { hunk, .. } => Some(*hunk),
        }
    }
}

/// Every file's diff, as one flat list of rows.
#[must_use]
pub fn diff_rows(files: &[FileDiff]) -> Vec<DiffRow> {
    let mut rows = Vec::new();
    let mut hunk = 0;

    for file in files {
        let (added, removed) = file.counts();
        rows.push(DiffRow::File {
            path: file.path.clone(),
            added,
            removed,
            binary: file.binary,
        });
        for one in &file.hunks {
            rows.push(DiffRow::Hunk {
                header: one.header(),
                hunk,
            });
            rows.extend(one.lines.iter().map(|line| DiffRow::Line {
                kind: line.kind,
                text: line.text.clone(),
                old: line.old,
                new: line.new,
                hunk,
            }));
            hunk += 1;
        }
    }
    rows
}
