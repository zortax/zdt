//! What git thinks has changed.
//!
//! One `git diff` per file, parsed into the three things a gutter shows: lines added, lines
//! changed, and places where something was taken out.
//!
//! A process rather than a library. Reading a repository properly means the index, the object
//! store, submodules, worktrees and whatever the person's `.gitattributes` says; git already does
//! all of that correctly, and the answer wanted here is forty bytes of unified diff.

use std::path::Path;

/// What happened to a run of lines.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Change {
    /// They are new.
    Added,
    /// They are different from what is committed.
    Changed,
    /// Something was taken out here. The mark sits on the line below the hole.
    Removed,
}

/// One run of lines that changed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hunk {
    /// The first line it covers, counting from zero the way an editor does.
    pub line: usize,
    /// How many lines. A removal covers none, and is drawn on one.
    pub count: usize,
    /// What happened.
    pub change: Change,
}

impl Hunk {
    /// Which lines it covers, as an editor would ask.
    #[must_use]
    pub fn lines(&self) -> std::ops::Range<usize> {
        self.line..self.line + self.count.max(1)
    }

    /// Whether `line` is in it.
    #[must_use]
    pub fn covers(&self, line: usize) -> bool {
        self.lines().contains(&line)
    }
}

/// What git says has changed in `path`.
///
/// Blocking; it runs a process. Nothing when the file is not in a repository, is not tracked, or
/// git is not installed — all of which are "no signs to draw" rather than errors worth reporting.
#[must_use]
pub fn hunks(path: &Path) -> Vec<Hunk> {
    let Some(directory) = path.parent() else {
        return Vec::new();
    };
    let Ok(output) = std::process::Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(["diff", "--no-color", "--no-ext-diff", "-U0", "--"])
        .arg(path)
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    parse(&String::from_utf8_lossy(&output.stdout))
}

/// The hunks in a unified diff produced with no context.
///
/// Only the `@@` lines matter at zero context: each says where the change is and how large it is
/// on each side, which is enough to tell the three kinds apart.
#[must_use]
pub fn parse(diff: &str) -> Vec<Hunk> {
    let mut hunks = Vec::new();

    for line in diff.lines() {
        let Some(rest) = line.strip_prefix("@@ ") else {
            continue;
        };
        let Some((before, after)) = header(rest) else {
            continue;
        };

        // The header counts from one; an editor counts from zero.
        let at = after.0.saturating_sub(1);
        match (before.1, after.1) {
            // Nothing on the old side: the lines are new.
            (0, added) if added > 0 => hunks.push(Hunk {
                line: at,
                count: added,
                change: Change::Added,
            }),
            // Nothing on the new side: something was taken out. The header points at the line
            // *before* the hole, so the mark goes on the one after it.
            (removed, 0) if removed > 0 => hunks.push(Hunk {
                line: after.0,
                count: 0,
                change: Change::Removed,
            }),
            (_, changed) if changed > 0 => hunks.push(Hunk {
                line: at,
                count: changed,
                change: Change::Changed,
            }),
            _ => {}
        }
    }

    hunks
}

/// The two `-a,b +c,d` halves of a hunk header, as (start, count) each.
fn header(rest: &str) -> Option<((usize, usize), (usize, usize))> {
    let mut parts = rest.split_whitespace();
    let before = side(parts.next()?.strip_prefix('-')?)?;
    let after = side(parts.next()?.strip_prefix('+')?)?;
    Some((before, after))
}

/// One `start,count` half. A half with no comma is one line.
fn side(text: &str) -> Option<(usize, usize)> {
    match text.split_once(',') {
        Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
        None => Some((text.parse().ok()?, 1)),
    }
}

/// The hunk `line` is in, or the next one after it, wrapping. What `]g` is.
#[must_use]
pub fn after(hunks: &[Hunk], line: usize) -> Option<Hunk> {
    hunks
        .iter()
        .find(|hunk| hunk.line > line)
        .or_else(|| hunks.first())
        .copied()
}

/// The one before it, wrapping. What `[g` is.
#[must_use]
pub fn before(hunks: &[Hunk], line: usize) -> Option<Hunk> {
    hunks
        .iter()
        .rev()
        .find(|hunk| hunk.line < line)
        .or_else(|| hunks.last())
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn added_lines_are_where_the_header_says() {
        // Two lines added after line 9 of the old file, appearing at line 10 of the new one.
        let hunks = parse("@@ -9,0 +10,2 @@\n+one\n+two\n");
        assert_eq!(
            hunks,
            vec![Hunk {
                line: 9,
                count: 2,
                change: Change::Added
            }]
        );
        assert_eq!(hunks[0].lines(), 9..11);
    }

    #[test]
    fn a_removal_is_marked_on_the_line_below_the_hole() {
        // Three lines taken out after line 4; there is nothing left to underline, so the mark goes
        // where the reader's eye is — the line that is now there.
        let hunks = parse("@@ -5,3 +4,0 @@\n-gone\n-gone\n-gone\n");
        assert_eq!(
            hunks,
            vec![Hunk {
                line: 4,
                count: 0,
                change: Change::Removed
            }]
        );
        assert_eq!(hunks[0].lines(), 4..5, "drawn on one line all the same");
    }

    #[test]
    fn a_changed_run_covers_the_new_lines() {
        let hunks = parse("@@ -3,2 +3,2 @@\n-old\n-old\n+new\n+new\n");
        assert_eq!(
            hunks,
            vec![Hunk {
                line: 2,
                count: 2,
                change: Change::Changed
            }]
        );
    }

    #[test]
    fn a_header_with_no_count_is_one_line() {
        let hunks = parse("@@ -0,0 +1 @@\n+only\n");
        assert_eq!(hunks[0].count, 1);
        assert_eq!(hunks[0].change, Change::Added);
    }

    #[test]
    fn several_hunks_come_back_in_order() {
        let diff = "\
diff --git a/f b/f
--- a/f
+++ b/f
@@ -1,0 +2,1 @@
+early
@@ -20,1 +22,1 @@
-old
+new
";
        let hunks = parse(diff);
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].line, 1);
        assert_eq!(hunks[1].line, 21);
    }

    #[test]
    fn everything_that_is_not_a_header_is_ignored() {
        let diff = "diff --git a/f b/f\nindex 0000..1111 100644\n--- a/f\n+++ b/f\n";
        assert!(parse(diff).is_empty());
        assert!(parse("").is_empty());
        assert!(parse("@@ nonsense @@").is_empty());
    }

    #[test]
    fn walking_wraps_at_the_ends() {
        let hunks = parse("@@ -0,0 +2,1 @@\n+a\n@@ -0,0 +9,1 @@\n+b\n");
        assert_eq!(hunks.len(), 2);

        assert_eq!(after(&hunks, 0).unwrap().line, 1);
        assert_eq!(after(&hunks, 1).unwrap().line, 8);
        assert_eq!(after(&hunks, 8).unwrap().line, 1, "past the last wraps");

        assert_eq!(before(&hunks, 8).unwrap().line, 1);
        assert_eq!(before(&hunks, 0).unwrap().line, 8, "and so does before");
    }

    #[test]
    fn walking_nothing_finds_nothing() {
        assert!(after(&[], 0).is_none());
        assert!(before(&[], 0).is_none());
    }

    #[test]
    fn a_hunk_knows_which_lines_it_covers() {
        let hunk = Hunk {
            line: 4,
            count: 3,
            change: Change::Changed,
        };
        assert!(hunk.covers(4));
        assert!(hunk.covers(6));
        assert!(!hunk.covers(7));
    }
}
