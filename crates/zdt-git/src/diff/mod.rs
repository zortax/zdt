//! What changed in a file, line by line.
//!
//! Three questions, all answered the same way and all needed by the panel:
//!
//!   * the working tree against the index, which is what is *not* staged.
//!   * the index against `HEAD`, which is what *is*.
//!   * one commit against its parent, which is what somebody did.
//!
//! # Why the lines and not just the hunks
//!
//! The gutter needs hunks: where something changed, and roughly what kind. A panel needs the text,
//! meaning the removed lines as well as the added ones, in order. That is what a diff *is*.
//! Staging one hunk needs the text too. Applying a hunk to the index-side blob means knowing
//! exactly which lines to put back and which to leave out.

mod text;

use crate::diff::text::{hunks_between, is_binary, lines_of};
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
    /// The whole old text, for a consumer that reads past the hunks. Shared, so a clone of the
    /// diff costs a reference count. Empty for a binary file, and for a file that is new.
    pub old_text: Option<std::sync::Arc<str>>,
    /// The whole new text. See `old_text`.
    pub new_text: Option<std::sync::Arc<str>>,
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
            old_text: None,
            new_text: None,
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

/// The unstaged changes to one file: what the working tree has and the index does not.
///
/// # Errors
///
/// When the file or the index cannot be read.
pub fn worktree(repo: &Repo, path: &str) -> Result<FileDiff, Error> {
    let old = index_blob(repo, path)?;
    let new = std::fs::read(repo.absolute(path)).unwrap_or_default();
    Ok(between(path, old.as_deref(), Some(&new)))
}

/// The staged changes to one file: what the index has and `HEAD` does not.
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
/// The first parent, so a merge shows what it brought onto the branch it was merged into. That is
/// the question somebody looking at a merge is asking.
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
        // The first commit came from nothing, which is an empty tree.
        None => git.empty_tree(),
    };

    trees(old_tree, new_tree)
}

/// What changed between two revisions, file by file.
///
/// Both name commits: branches, checkpoint references, or ids. The answer is `new` against
/// `old`, which is what a review of the span between them asks.
///
/// # Errors
///
/// When either revision cannot be read.
pub fn commits(repo: &Repo, old: &str, new: &str) -> Result<Vec<FileDiff>, Error> {
    let git = repo.git();
    let old_tree = git
        .rev_parse_single(old)
        .map_err(Error::git)?
        .object()
        .map_err(Error::git)?
        .peel_to_commit()
        .map_err(Error::git)?
        .tree()
        .map_err(Error::git)?;
    let new_tree = git
        .rev_parse_single(new)
        .map_err(Error::git)?
        .object()
        .map_err(Error::git)?
        .peel_to_commit()
        .map_err(Error::git)?
        .tree()
        .map_err(Error::git)?;
    trees(old_tree, new_tree)
}

/// What changed from one tree to another, file by file and in path order.
fn trees(old_tree: gix::Tree<'_>, new_tree: gix::Tree<'_>) -> Result<Vec<FileDiff>, Error> {
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
            old_text: None,
            new_text: None,
        };
    }

    let old_text: Option<std::sync::Arc<str>> =
        old.map(|bytes| String::from_utf8_lossy(bytes).into());
    let new_text: Option<std::sync::Arc<str>> =
        new.map(|bytes| String::from_utf8_lossy(bytes).into());
    let old_lines = lines_of(old.unwrap_or_default());
    let new_lines = lines_of(new.unwrap_or_default());
    FileDiff {
        path: path.to_owned(),
        from: None,
        hunks: hunks_between(&old_lines, &new_lines),
        binary: false,
        old_text,
        new_text,
    }
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
mod tests;
