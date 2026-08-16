//! What is changed, staged, and untracked.

use std::path::PathBuf;

use crate::repo::{Error, Repo};

/// What has happened to one path.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum State {
    /// It is as it was.
    #[default]
    Unchanged,
    /// It is not in the repository at all.
    Untracked,
    /// It is new here.
    Added,
    /// Its contents differ.
    Modified,
    /// It is gone.
    Deleted,
    /// It was moved.
    Renamed,
    /// A merge left it with two answers.
    Conflicted,
}

impl State {
    /// The one-letter mark git itself would print, which is what a person reads a status by.
    #[must_use]
    pub const fn mark(self) -> &'static str {
        match self {
            Self::Unchanged => " ",
            Self::Untracked => "?",
            Self::Added => "A",
            Self::Modified => "M",
            Self::Deleted => "D",
            Self::Renamed => "R",
            Self::Conflicted => "U",
        }
    }

    /// Whether anything happened at all.
    #[must_use]
    pub const fn is_change(self) -> bool {
        !matches!(self, Self::Unchanged)
    }
}

/// One path, and what has happened to it on each side.
///
/// Two states rather than one, because that is the shape of the thing: a file can be staged *and*
/// changed again since, and a panel that showed one letter could not say so — which is exactly the
/// case where somebody is about to commit half of what they think they are committing.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    /// Which file, as git names it.
    pub path: String,
    /// Where it is on this machine.
    pub full: PathBuf,
    /// What the index says about it, against the last commit.
    pub index: State,
    /// What the working tree says about it, against the index.
    pub worktree: State,
    /// Where it came from, when it was moved.
    pub from: Option<String>,
}

impl Entry {
    /// Whether any of it is staged.
    #[must_use]
    pub const fn is_staged(&self) -> bool {
        self.index.is_change()
    }

    /// Whether any of it is not.
    #[must_use]
    pub const fn is_unstaged(&self) -> bool {
        self.worktree.is_change()
    }

    /// Whether a merge left it needing an answer.
    #[must_use]
    pub const fn is_conflicted(&self) -> bool {
        matches!(self.index, State::Conflicted) || matches!(self.worktree, State::Conflicted)
    }
}

/// Everything git would list, in the order it lists it.
///
/// Untracked files are included but directories are not walked into recursively past the first
/// untracked one — the same rule `git status` uses, and for the same reason: a fresh `target/` is
/// one row rather than nine thousand.
///
/// # Errors
///
/// When the status cannot be worked out at all.
pub fn status(repo: &Repo) -> Result<Vec<Entry>, Error> {
    let mut found: Vec<Entry> = Vec::new();

    let iter = repo
        .git()
        .status(gix::progress::Discard)
        .map_err(Error::git)?
        .index_worktree_options_mut(|options| {
            // A fresh `target/` is one row rather than nine thousand, which is the same rule
            // `git status` itself follows and for the same reason.
            if let Some(dirwalk) = options.dirwalk_options.as_mut() {
                dirwalk.set_emit_untracked(gix::dir::walk::EmissionMode::CollapseDirectory);
            }
        })
        .into_index_worktree_iter(Vec::new())
        .map_err(Error::git)?;

    for item in iter {
        let item = item.map_err(Error::git)?;
        // `None` is an item that is not a change at all — an index entry whose timestamp wants
        // refreshing, or a walked file that turned out to be tracked and unmodified.
        let Some(summary) = item.summary() else {
            continue;
        };
        let path = item.rela_path().to_string();
        found.push(Entry {
            full: repo.absolute(&path),
            path,
            index: State::Unchanged,
            worktree: worktree_state(summary, &item),
            from: None,
        });
    }

    // What the index holds that the last commit does not. A second pass because the two questions
    // are asked of two different things — the tree against the index, and the index against the
    // working copy — and gix answers them separately.
    for (path, state) in staged(repo)? {
        match found.iter_mut().find(|entry| entry.path == path) {
            Some(entry) => entry.index = state,
            None => found.push(Entry {
                full: repo.absolute(&path),
                path,
                index: state,
                worktree: State::Unchanged,
                from: None,
            }),
        }
    }

    found.sort_by(|a, b| a.path.cmp(&b.path));
    found.dedup_by(|a, b| a.path == b.path);
    Ok(found)
}

/// What the index holds that `HEAD` does not.
fn staged(repo: &Repo) -> Result<Vec<(String, State)>, Error> {
    use gix::diff::index::ChangeRef as Change;

    let Ok(head) = repo.git().head_tree_id() else {
        // No commits yet, so everything in the index is new.
        return staged_against_nothing(repo);
    };

    let mut out = Vec::new();
    let index = repo.git().index_or_empty().map_err(Error::git)?;

    repo.git()
        .tree_index_status(
            &head,
            &index,
            None,
            gix::status::tree_index::TrackRenames::Disabled,
            |change, _, _| {
                let (path, state) = match &change {
                    Change::Addition { location, .. } => (location.to_string(), State::Added),
                    Change::Deletion { location, .. } => (location.to_string(), State::Deleted),
                    Change::Modification { location, .. } => {
                        (location.to_string(), State::Modified)
                    }
                    Change::Rewrite { location, .. } => (location.to_string(), State::Renamed),
                };
                out.push((path, state));
                Ok::<_, std::convert::Infallible>(std::ops::ControlFlow::Continue(()))
            },
        )
        .map_err(Error::git)?;

    Ok(out)
}

/// The same, for a repository with no commits in it.
fn staged_against_nothing(repo: &Repo) -> Result<Vec<(String, State)>, Error> {
    let index = repo.git().index_or_empty().map_err(Error::git)?;
    Ok(index
        .entries()
        .iter()
        .map(|entry| (entry.path(&index).to_string(), State::Added))
        .collect())
}

/// What one working-tree item amounts to.
///
/// `Added` means two different things depending on where the item came from: a file the walk found
/// that the index has never heard of is untracked, while an index entry marked intent-to-add is a
/// file git has been told about but has no content for. A panel that called both "added" would
/// offer to unstage something that was never staged.
fn worktree_state(
    summary: gix::status::index_worktree::iter::Summary,
    item: &gix::status::index_worktree::Item,
) -> State {
    use gix::status::index_worktree::Item;
    use gix::status::index_worktree::iter::Summary;

    match summary {
        Summary::Removed => State::Deleted,
        Summary::Added => match item {
            Item::DirectoryContents { .. } => State::Untracked,
            _ => State::Added,
        },
        Summary::Modified | Summary::TypeChange => State::Modified,
        Summary::Renamed | Summary::Copied => State::Renamed,
        Summary::IntentToAdd => State::Added,
        Summary::Conflict => State::Conflicted,
    }
}
