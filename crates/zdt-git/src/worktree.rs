//! Worktrees: parallel checkouts of one repository.
//!
//! An agent thread that must not disturb the main checkout works in a worktree of its own, on a
//! branch of its own. Everything here runs `git worktree`, because a worktree is bookkeeping the
//! command gets right by definition and this happens once per thread, never per keystroke.

use std::path::{Path, PathBuf};

use crate::repo::{Error, Repo, git};

/// One worktree, as `git worktree list` reports it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Worktree {
    /// Where its checkout is.
    pub path: PathBuf,
    /// The branch checked out in it. Empty for a detached one.
    pub branch: String,
}

/// A fresh temporary branch name: `zdt/` and eight hex characters.
///
/// Given to a worktree at creation; a later pass renames it to something a person would write.
#[must_use]
pub fn temp_branch() -> String {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let mixed = seed ^ u128::from(std::process::id()) << 64;
    format!("zdt/{:08x}", (mixed as u32) ^ ((mixed >> 32) as u32))
}

/// Fetches `branch` from `origin`, so a new worktree can start from the remote's head.
///
/// # Errors
///
/// When there is no such remote or branch, or the network says no.
pub fn fetch(repo: &Repo, branch: &str) -> Result<(), Error> {
    git(repo, &["fetch", "--quiet", "origin", branch], &[])?;
    Ok(())
}

/// Adds a worktree at `path`, on a new branch `branch` starting from `base`.
///
/// The parent directory is made when it is missing. `base` is any revision: a local branch, a
/// remote-tracking one, or a commit.
///
/// # Errors
///
/// When the branch exists already, `base` names nothing, or the checkout cannot be written.
pub fn add(repo: &Repo, path: &Path, branch: &str, base: &str) -> Result<Worktree, Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| Error::Git(format!("{}: {error}", parent.display())))?;
    }
    let spot = path.to_string_lossy();
    git(
        repo,
        &["worktree", "add", "--quiet", "-b", branch, &spot, base],
        &[],
    )?;
    Ok(Worktree {
        path: path.to_path_buf(),
        branch: branch.to_owned(),
    })
}

/// Removes the worktree at `path`, and its branch when one is named.
///
/// Forced: an agent's worktree holds work the checkpoints already captured, and a removal the
/// person asked for must not stall on a dirty tree. A path that is already gone is pruned from
/// the bookkeeping and counts as removed.
///
/// # Errors
///
/// When git refuses for any other reason.
pub fn remove(repo: &Repo, path: &Path, branch: Option<&str>) -> Result<(), Error> {
    let spot = path.to_string_lossy();
    let removed = git(repo, &["worktree", "remove", "--force", &spot], &[]);
    if removed.is_err() && path.exists() {
        return removed.map(|_| ());
    }
    let _ = git(repo, &["worktree", "prune"], &[]);
    if let Some(branch) = branch {
        let _ = git(repo, &["branch", "-D", branch], &[]);
    }
    Ok(())
}

/// Renames a branch, wherever it is checked out.
///
/// `git branch -m` moves the ref and repoints every worktree HEAD that named it. Refused when a
/// branch called `to` exists already.
///
/// # Errors
///
/// When `from` names nothing, `to` is taken, or git refuses.
pub fn rename_branch(repo: &Repo, from: &str, to: &str) -> Result<(), Error> {
    git(repo, &["branch", "-m", from, to], &[])?;
    Ok(())
}

/// Every worktree of the repository, the main checkout excluded.
///
/// # Errors
///
/// When the listing cannot be read.
pub fn list(repo: &Repo) -> Result<Vec<Worktree>, Error> {
    let said = git(repo, &["worktree", "list", "--porcelain"], &[])?;
    let mut found = Vec::new();
    let mut path: Option<PathBuf> = None;
    let mut branch = String::new();
    for line in said.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if let Some(done) = path.take()
                && done != repo.root()
            {
                found.push(Worktree {
                    path: done,
                    branch: std::mem::take(&mut branch),
                });
            }
            branch.clear();
        } else if let Some(spot) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(spot));
        } else if let Some(name) = line.strip_prefix("branch refs/heads/") {
            branch = name.to_owned();
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::{add, list, remove, temp_branch};
    use crate::repo::testing::Temp;

    #[test]
    fn a_temporary_branch_name_is_a_zdt_one() {
        let name = temp_branch();
        assert!(name.starts_with("zdt/"), "{name}");
        assert_eq!(name.len(), 12, "{name}");
    }

    #[test]
    fn a_worktree_is_added_on_its_own_branch_and_listed() {
        let temp = Temp::new("worktree-add");
        temp.commit("a.txt", "one\n", "first");
        let repo = temp.repo();

        let spot = temp.root().join("trees").join("side");
        let made = add(&repo, &spot, "zdt/abc12345", "main").expect("it adds");
        assert_eq!(made.branch, "zdt/abc12345");
        assert!(spot.join("a.txt").exists(), "the checkout is there");

        let found = list(&repo).expect("it lists");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].branch, "zdt/abc12345");
    }

    #[test]
    fn removing_a_worktree_takes_its_branch_with_it() {
        let temp = Temp::new("worktree-remove");
        temp.commit("a.txt", "one\n", "first");
        let repo = temp.repo();

        let spot = temp.root().join("trees").join("gone");
        add(&repo, &spot, "zdt/dead0000", "main").expect("it adds");
        // Dirty on purpose: removal is forced.
        temp.write("trees/gone/b.txt", "loose\n");

        remove(&repo, &spot, Some("zdt/dead0000")).expect("it removes");
        assert!(!spot.exists());
        assert!(list(&repo).expect("it lists").is_empty());
        assert!(
            !temp.run(&["branch", "--list"]).contains("zdt/dead0000"),
            "the branch is gone too"
        );
    }

    #[test]
    fn a_worktree_whose_directory_was_deleted_by_hand_still_removes() {
        let temp = Temp::new("worktree-pruned");
        temp.commit("a.txt", "one\n", "first");
        let repo = temp.repo();

        let spot = temp.root().join("trees").join("lost");
        add(&repo, &spot, "zdt/beef0000", "main").expect("it adds");
        std::fs::remove_dir_all(&spot).expect("taken away by hand");

        remove(&repo, &spot, Some("zdt/beef0000")).expect("it still removes");
        assert!(list(&repo).expect("it lists").is_empty());
    }

    #[test]
    fn a_worktree_branch_renames_and_the_checkout_follows() {
        let temp = Temp::new("worktree-rename");
        temp.commit("a.txt", "one\n", "first");
        let repo = temp.repo();

        let spot = temp.root().join("trees").join("named");
        add(&repo, &spot, "zdt/cafe0000", "main").expect("it adds");
        super::rename_branch(&repo, "zdt/cafe0000", "zdt/fix-the-thing").expect("it renames");

        let found = list(&repo).expect("it lists");
        assert_eq!(found[0].branch, "zdt/fix-the-thing");
    }

    #[test]
    fn a_branch_that_exists_already_is_refused() {
        let temp = Temp::new("worktree-taken");
        temp.commit("a.txt", "one\n", "first");
        temp.run(&["branch", "taken"]);
        let repo = temp.repo();

        let spot = temp.root().join("trees").join("dup");
        assert!(add(&repo, &spot, "taken", "main").is_err());
    }
}
