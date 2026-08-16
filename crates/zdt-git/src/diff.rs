//! What changed in a file, line by line.
//!
//! Three questions, all answered the same way and all needed by the panel:
//!
//!   * the working tree against the index — what is *not* staged;
//!   * the index against `HEAD` — what *is*;
//!   * one commit against its parent — what somebody did.
//!
//! # Why the lines and not just the hunks
//!
//! The gutter needs hunks: where something changed, and roughly what kind. A panel needs the text
//! — the removed lines as well as the added ones, in order, because that is what a diff *is*. And
//! staging one hunk needs the text too: applying a hunk to the index-side blob means knowing
//! exactly which lines to put back and which to leave out.

use crate::repo::{Error, Repo};

/// What happened to one line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineKind {
    /// It is in both, unchanged.
    Context,
    /// It is only in the new text.
    Added,
    /// It is only in the old text.
    Removed,
}

/// One line of a diff.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Line {
    /// What happened to it.
    pub kind: LineKind,
    /// The text, without its newline.
    pub text: String,
    /// Which line it is in the old file, counting from one. `None` for a line that is only new.
    pub old: Option<u32>,
    /// Which line it is in the new file. `None` for a line that is only old.
    pub new: Option<u32>,
}

/// One run of changed lines, and the unchanged ones around it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DiffHunk {
    /// Where it starts in the old file, counting from one.
    pub old_start: u32,
    /// How many lines of the old file it covers.
    pub old_count: u32,
    /// Where it starts in the new file.
    pub new_start: u32,
    /// How many lines of the new file it covers.
    pub new_count: u32,
    /// The lines themselves, in the order they are drawn.
    pub lines: Vec<Line>,
}

impl DiffHunk {
    /// The `@@` line git would print for it.
    #[must_use]
    pub fn header(&self) -> String {
        format!(
            "@@ -{},{} +{},{} @@",
            self.old_start, self.old_count, self.new_start, self.new_count
        )
    }

    /// How many lines it adds and how many it takes away.
    #[must_use]
    pub fn counts(&self) -> (usize, usize) {
        let added = self
            .lines
            .iter()
            .filter(|line| line.kind == LineKind::Added)
            .count();
        let removed = self
            .lines
            .iter()
            .filter(|line| line.kind == LineKind::Removed)
            .count();
        (added, removed)
    }
}

/// The whole of what changed in one file.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FileDiff {
    /// Which file, as git names it.
    pub path: String,
    /// What it was called before, when it was moved.
    pub from: Option<String>,
    /// The hunks, in file order.
    pub hunks: Vec<DiffHunk>,
    /// Whether either side is not text.
    ///
    /// A binary file has no lines to show, and a panel that tried would draw a screenful of
    /// replacement characters.
    pub binary: bool,
}

impl FileDiff {
    /// Nothing changed.
    #[must_use]
    pub fn empty(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            from: None,
            hunks: Vec::new(),
            binary: false,
        }
    }

    /// How many lines it adds and how many it takes away, across every hunk.
    #[must_use]
    pub fn counts(&self) -> (usize, usize) {
        self.hunks.iter().fold((0, 0), |(added, removed), hunk| {
            let (a, r) = hunk.counts();
            (added + a, removed + r)
        })
    }

    /// Whether anything changed at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.hunks.is_empty()
    }
}

/// How much unchanged text is shown around a change.
///
/// Three lines, which is what git shows and what a person needs to recognise where they are.
const CONTEXT: u32 = 3;

/// What the working tree has that the index does not — the unstaged changes to one file.
///
/// # Errors
///
/// When the file or the index cannot be read.
pub fn worktree(repo: &Repo, path: &str) -> Result<FileDiff, Error> {
    let old = index_blob(repo, path)?;
    let new = std::fs::read(repo.absolute(path)).unwrap_or_default();
    Ok(between(path, old.as_deref(), Some(&new)))
}

/// What the index has that `HEAD` does not — the staged changes to one file.
///
/// # Errors
///
/// When the index or the commit cannot be read.
pub fn staged(repo: &Repo, path: &str) -> Result<FileDiff, Error> {
    let old = head_blob(repo, path)?;
    let new = index_blob(repo, path)?;
    Ok(between(path, old.as_deref(), new.as_deref()))
}

/// What one commit changed, against its first parent.
///
/// The first parent, so a merge shows what it brought onto the branch it was merged into rather
/// than everything on both sides — which is the question somebody looking at a merge is asking.
///
/// # Errors
///
/// When the commit cannot be read.
pub fn commit(repo: &Repo, revision: &str) -> Result<Vec<FileDiff>, Error> {
    let git = repo.git();
    let id = git.rev_parse_single(revision).map_err(Error::git)?.detach();
    let object = git.find_commit(id).map_err(Error::git)?;
    let new_tree = object.tree().map_err(Error::git)?;

    let old_tree = match object.parent_ids().next() {
        Some(parent) => git
            .find_commit(parent.detach())
            .map_err(Error::git)?
            .tree()
            .map_err(Error::git)?,
        // The first commit came from nothing, which is an empty tree rather than an error.
        None => git.empty_tree(),
    };

    // One entry per file the commit touched: what it is called, and its bytes on each side.
    type Touched = (String, Option<Vec<u8>>, Option<Vec<u8>>);
    let mut changes: Vec<Touched> = Vec::new();
    old_tree
        .changes()
        .map_err(Error::git)?
        .for_each_to_obtain_tree(&new_tree, |change| {
            use gix::object::tree::diff::Change;

            let (path, before, after) = match change {
                Change::Addition { location, id, .. } => (
                    location.to_string(),
                    None,
                    id.object().ok().map(|object| object.data.clone()),
                ),
                Change::Deletion { location, id, .. } => (
                    location.to_string(),
                    id.object().ok().map(|object| object.data.clone()),
                    None,
                ),
                Change::Modification {
                    location,
                    previous_id,
                    id,
                    ..
                } => (
                    location.to_string(),
                    previous_id.object().ok().map(|object| object.data.clone()),
                    id.object().ok().map(|object| object.data.clone()),
                ),
                Change::Rewrite {
                    location,
                    source_id,
                    id,
                    ..
                } => (
                    location.to_string(),
                    source_id.object().ok().map(|object| object.data.clone()),
                    id.object().ok().map(|object| object.data.clone()),
                ),
            };
            changes.push((path, before, after));
            Ok::<_, std::convert::Infallible>(std::ops::ControlFlow::Continue(()))
        })
        .map_err(Error::git)?;

    let mut out: Vec<FileDiff> = changes
        .into_iter()
        .map(|(path, before, after)| between(&path, before.as_deref(), after.as_deref()))
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// Which files a commit touched, without reading their contents.
///
/// For the list beside a commit's details, which is drawn before any one file is chosen.
///
/// # Errors
///
/// As [`commit`].
pub fn commit_files(repo: &Repo, revision: &str) -> Result<Vec<FileDiff>, Error> {
    commit(repo, revision)
}

/// The diff between two texts.
///
/// Either side may be missing, which is a file added or taken away.
#[must_use]
pub fn between(path: &str, old: Option<&[u8]>, new: Option<&[u8]>) -> FileDiff {
    let binary = old.is_some_and(is_binary) || new.is_some_and(is_binary);
    if binary {
        return FileDiff {
            path: path.to_owned(),
            from: None,
            hunks: Vec::new(),
            binary: true,
        };
    }

    let old_lines = lines_of(old.unwrap_or_default());
    let new_lines = lines_of(new.unwrap_or_default());
    FileDiff {
        path: path.to_owned(),
        from: None,
        hunks: hunks_between(&old_lines, &new_lines),
        binary: false,
    }
}

/// A blob's bytes as lines, without their newlines.
fn lines_of(bytes: &[u8]) -> Vec<String> {
    if bytes.is_empty() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(bytes);
    let mut lines: Vec<String> = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
        .collect();
    // A file ending in a newline splits into a trailing empty piece that is not a line.
    if text.ends_with('\n') {
        lines.pop();
    }
    lines
}

/// Whether a blob is something with lines in it.
///
/// A zero byte in the first eight kilobytes, which is the rule git itself uses. Not a guess about
/// encodings: what this decides is whether showing the thing as text would be a screenful of
/// replacement characters.
fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8000).any(|byte| *byte == 0)
}

/// The hunks between two lists of lines.
fn hunks_between(old: &[String], new: &[String]) -> Vec<DiffHunk> {
    let script = myers(old, new);
    if script.iter().all(|step| matches!(step, Step::Keep(_, _))) {
        return Vec::new();
    }

    // Group the script into runs of changes with up to `CONTEXT` unchanged lines around them,
    // joining two runs that are close enough that the context between them would overlap.
    let changed: Vec<usize> = script
        .iter()
        .enumerate()
        .filter(|(_, step)| !matches!(step, Step::Keep(_, _)))
        .map(|(at, _)| at)
        .collect();

    let mut groups: Vec<(usize, usize)> = Vec::new();
    for at in changed {
        let from = at.saturating_sub(CONTEXT as usize);
        let to = (at + CONTEXT as usize + 1).min(script.len());
        match groups.last_mut() {
            Some((_, end)) if *end >= from => *end = to.max(*end),
            _ => groups.push((from, to)),
        }
    }

    groups
        .into_iter()
        .map(|(from, to)| {
            let mut lines = Vec::new();
            let (mut old_start, mut new_start) = (0, 0);
            let (mut old_count, mut new_count) = (0, 0);
            let mut started = false;

            for step in &script[from..to] {
                let line = match step {
                    Step::Keep(o, n) => {
                        old_count += 1;
                        new_count += 1;
                        Line {
                            kind: LineKind::Context,
                            text: old[*o].clone(),
                            old: Some(*o as u32 + 1),
                            new: Some(*n as u32 + 1),
                        }
                    }
                    Step::Remove(o) => {
                        old_count += 1;
                        Line {
                            kind: LineKind::Removed,
                            text: old[*o].clone(),
                            old: Some(*o as u32 + 1),
                            new: None,
                        }
                    }
                    Step::Add(n) => {
                        new_count += 1;
                        Line {
                            kind: LineKind::Added,
                            text: new[*n].clone(),
                            old: None,
                            new: Some(*n as u32 + 1),
                        }
                    }
                };
                if !started {
                    old_start = line.old.unwrap_or(0);
                    new_start = line.new.unwrap_or(0);
                    started = true;
                }
                // A hunk that opens on an added line still starts somewhere in the old file, and
                // that somewhere is wherever the next old line is.
                if old_start == 0
                    && let Some(old) = line.old
                {
                    old_start = old;
                }
                if new_start == 0
                    && let Some(new) = line.new
                {
                    new_start = new;
                }
                lines.push(line);
            }

            DiffHunk {
                old_start: old_start.max(1),
                old_count,
                new_start: new_start.max(1),
                new_count,
                lines,
            }
        })
        .collect()
}

/// One step of an edit script.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Step {
    /// A line in both, at these two places.
    Keep(usize, usize),
    /// A line only in the old text.
    Remove(usize),
    /// A line only in the new text.
    Add(usize),
}

/// The edit script between two lists of lines.
///
/// A longest-common-subsequence diff, with the common prefix and suffix taken off first — which is
/// what makes it fast enough on real files: a one-line change in a two-thousand-line file leaves
/// a handful of lines for the quadratic part to work on.
fn myers(old: &[String], new: &[String]) -> Vec<Step> {
    let head = old
        .iter()
        .zip(new.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let tail = old[head..]
        .iter()
        .rev()
        .zip(new[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();

    let mut script: Vec<Step> = (0..head).map(|at| Step::Keep(at, at)).collect();

    let (old_middle, new_middle) = (&old[head..old.len() - tail], &new[head..new.len() - tail]);
    script.extend(
        lcs(old_middle, new_middle)
            .into_iter()
            .map(|step| match step {
                Step::Keep(o, n) => Step::Keep(o + head, n + head),
                Step::Remove(o) => Step::Remove(o + head),
                Step::Add(n) => Step::Add(n + head),
            }),
    );

    for at in 0..tail {
        script.push(Step::Keep(old.len() - tail + at, new.len() - tail + at));
    }
    script
}

/// The edit script between two lists that share no prefix or suffix.
fn lcs(old: &[String], new: &[String]) -> Vec<Step> {
    if old.is_empty() {
        return (0..new.len()).map(Step::Add).collect();
    }
    if new.is_empty() {
        return (0..old.len()).map(Step::Remove).collect();
    }

    // A table of how long the common subsequence is from each pair of positions onward. Quadratic,
    // which is why the prefix and suffix are stripped before this is reached; a change large
    // enough for this to matter is one nobody is reading line by line anyway.
    let (rows, columns) = (old.len() + 1, new.len() + 1);
    let mut table = vec![0u32; rows * columns];
    for o in (0..old.len()).rev() {
        for n in (0..new.len()).rev() {
            table[o * columns + n] = if old[o] == new[n] {
                table[(o + 1) * columns + n + 1] + 1
            } else {
                table[(o + 1) * columns + n].max(table[o * columns + n + 1])
            };
        }
    }

    let mut script = Vec::new();
    let (mut o, mut n) = (0, 0);
    while o < old.len() && n < new.len() {
        if old[o] == new[n] {
            script.push(Step::Keep(o, n));
            o += 1;
            n += 1;
        } else if table[(o + 1) * columns + n] >= table[o * columns + n + 1] {
            script.push(Step::Remove(o));
            o += 1;
        } else {
            script.push(Step::Add(n));
            n += 1;
        }
    }
    while o < old.len() {
        script.push(Step::Remove(o));
        o += 1;
    }
    while n < new.len() {
        script.push(Step::Add(n));
        n += 1;
    }
    script
}

/// What the index holds for `path`, when it holds anything.
pub(crate) fn index_blob(repo: &Repo, path: &str) -> Result<Option<Vec<u8>>, Error> {
    let index = repo.git().index_or_empty().map_err(Error::git)?;
    let Some(entry) = index.entry_by_path(path.into()) else {
        return Ok(None);
    };
    let object = repo.git().find_object(entry.id).map_err(Error::git)?;
    Ok(Some(object.data.clone()))
}

/// What the last commit holds for `path`, when it holds anything.
pub(crate) fn head_blob(repo: &Repo, path: &str) -> Result<Option<Vec<u8>>, Error> {
    let git = repo.git();
    let Ok(id) = git.head_tree_id() else {
        return Ok(None);
    };
    let mut tree = git.find_tree(id).map_err(Error::git)?;
    let Ok(Some(entry)) = tree.peel_to_entry_by_path(path) else {
        return Ok(None);
    };
    let object = entry.object().map_err(Error::git)?;
    Ok(Some(object.data.clone()))
}

#[cfg(test)]
mod tests {
    use super::{DiffHunk, LineKind, between, is_binary, lines_of, staged, worktree};
    use crate::repo::testing::Temp;

    /// The kinds of the lines of one hunk, as a string: `.` context, `+` added, `-` removed.
    fn shape(hunk: &DiffHunk) -> String {
        hunk.lines
            .iter()
            .map(|line| match line.kind {
                LineKind::Context => '.',
                LineKind::Added => '+',
                LineKind::Removed => '-',
            })
            .collect()
    }

    #[test]
    fn a_file_that_did_not_change_has_no_hunks() {
        let found = between("a.txt", Some(b"one\ntwo\n"), Some(b"one\ntwo\n"));
        assert!(found.is_empty());
        assert_eq!(found.counts(), (0, 0));
    }

    #[test]
    fn one_line_changed_is_one_hunk() {
        let found = between(
            "a.txt",
            Some(b"one\ntwo\nthree\n"),
            Some(b"one\nTWO\nthree\n"),
        );
        assert_eq!(found.hunks.len(), 1);
        assert_eq!(found.counts(), (1, 1));
        assert_eq!(shape(&found.hunks[0]), ".-+.");
    }

    #[test]
    fn a_new_file_is_all_additions() {
        let found = between("a.txt", None, Some(b"one\ntwo\n"));
        assert_eq!(found.counts(), (2, 0));
        assert_eq!(shape(&found.hunks[0]), "++");
        assert_eq!(found.hunks[0].new_start, 1);
    }

    #[test]
    fn a_deleted_file_is_all_removals() {
        let found = between("a.txt", Some(b"one\ntwo\n"), None);
        assert_eq!(found.counts(), (0, 2));
        assert_eq!(shape(&found.hunks[0]), "--");
    }

    #[test]
    fn the_line_numbers_are_the_ones_git_would_print() {
        // Four lines, the third changed. Git says `@@ -2,3 +2,3 @@` — three lines of context
        // around it, starting at the second.
        let old = b"a\nb\nc\nd\ne\nf\ng\n";
        let new = b"a\nb\nc\nD\ne\nf\ng\n";
        let found = between("a.txt", Some(old), Some(new));
        let hunk = &found.hunks[0];
        assert_eq!(hunk.old_start, 1);
        assert_eq!(hunk.new_start, 1);
        assert_eq!(shape(hunk), "...-+...");

        // And every line says where it is on each side.
        let changed = hunk
            .lines
            .iter()
            .find(|line| line.kind == LineKind::Added)
            .expect("one");
        assert_eq!(changed.new, Some(4));
        assert_eq!(changed.old, None);
    }

    #[test]
    fn two_changes_far_apart_are_two_hunks() {
        let old: String = (0..40).map(|n| format!("line {n}\n")).collect();
        let mut new_lines: Vec<String> = (0..40).map(|n| format!("line {n}")).collect();
        new_lines[2] = "changed near the top".to_owned();
        new_lines[35] = "changed near the bottom".to_owned();
        let new: String = new_lines.iter().map(|line| format!("{line}\n")).collect();

        let found = between("a.txt", Some(old.as_bytes()), Some(new.as_bytes()));
        assert_eq!(found.hunks.len(), 2, "far apart, so not joined");
    }

    #[test]
    fn two_changes_close_together_are_one_hunk() {
        // Because their context would overlap, and two hunks sharing lines would draw them twice.
        let old: String = (0..20).map(|n| format!("line {n}\n")).collect();
        let mut new_lines: Vec<String> = (0..20).map(|n| format!("line {n}")).collect();
        new_lines[8] = "one".to_owned();
        new_lines[10] = "two".to_owned();
        let new: String = new_lines.iter().map(|line| format!("{line}\n")).collect();

        let found = between("a.txt", Some(old.as_bytes()), Some(new.as_bytes()));
        assert_eq!(found.hunks.len(), 1);
    }

    #[test]
    fn a_binary_file_says_so_rather_than_showing_itself() {
        let found = between("a.png", Some(b"\x89PNG\x00\x01"), Some(b"\x89PNG\x00\x02"));
        assert!(found.binary);
        assert!(found.hunks.is_empty(), "there is nothing to draw");
    }

    #[test]
    fn what_counts_as_binary() {
        assert!(is_binary(b"before\x00after"));
        assert!(!is_binary(b"plain text\nwith lines\n"));
        assert!(!is_binary(b""), "an empty file is text");
    }

    #[test]
    fn a_file_with_no_trailing_newline_has_no_phantom_last_line() {
        assert_eq!(lines_of(b"one\ntwo"), ["one", "two"]);
        assert_eq!(lines_of(b"one\ntwo\n"), ["one", "two"]);
        assert!(lines_of(b"").is_empty());
    }

    #[test]
    fn carriage_returns_are_not_part_of_the_line() {
        // Otherwise every line of a file checked out with CRLF endings reads as changed.
        assert_eq!(lines_of(b"one\r\ntwo\r\n"), ["one", "two"]);
    }

    #[test]
    fn the_working_tree_diff_is_what_is_not_staged() {
        let temp = Temp::new("diff-worktree");
        temp.commit("a.txt", "one\ntwo\n", "first");
        temp.write("a.txt", "one\nTWO\n");

        let found = worktree(&temp.repo(), "a.txt").expect("it diffs");
        assert_eq!(found.counts(), (1, 1));

        // Staged, and there is nothing left unstaged.
        temp.run(&["add", "a.txt"]);
        let found = worktree(&temp.repo(), "a.txt").expect("it diffs");
        assert!(found.is_empty(), "everything is in the index now");
    }

    #[test]
    fn the_staged_diff_is_what_is_staged() {
        let temp = Temp::new("diff-staged");
        temp.commit("a.txt", "one\ntwo\n", "first");

        let found = staged(&temp.repo(), "a.txt").expect("it diffs");
        assert!(found.is_empty(), "nothing is staged yet");

        temp.write("a.txt", "one\nTWO\n");
        temp.run(&["add", "a.txt"]);
        let found = staged(&temp.repo(), "a.txt").expect("it diffs");
        assert_eq!(found.counts(), (1, 1));
    }

    #[test]
    fn a_commit_says_what_it_changed() {
        let temp = Temp::new("diff-commit");
        temp.commit("a.txt", "one\n", "first");
        temp.commit("a.txt", "one\ntwo\n", "second");

        let found = super::commit(&temp.repo(), "HEAD").expect("it diffs");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, "a.txt");
        assert_eq!(found[0].counts(), (1, 0));
    }

    #[test]
    fn the_first_commit_is_diffed_against_nothing() {
        // Which is every file in it added, rather than an error about a parent that is not there.
        let temp = Temp::new("diff-first");
        temp.commit("a.txt", "one\ntwo\n", "first");

        let found = super::commit(&temp.repo(), "HEAD").expect("it diffs");
        assert_eq!(found[0].counts(), (2, 0));
    }

    #[test]
    fn a_hunk_header_reads_as_git_writes_it() {
        let found = between("a.txt", Some(b"one\n"), Some(b"two\n"));
        assert_eq!(found.hunks[0].header(), "@@ -1,1 +1,1 @@");
    }
}
