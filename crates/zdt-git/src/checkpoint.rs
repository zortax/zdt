//! Checkpoints: the working tree, captured around an agent turn.
//!
//! A checkpoint is a parentless hidden commit of everything in the working tree, tracked or not,
//! written through a temporary index so the person's own index is never touched. One sits before
//! each turn and one after, which is what per-turn diffs and revert are made of.
//!
//! The refs live under `refs/zdt/checkpoints/`, where no porcelain lists them.

use std::path::PathBuf;

use crate::diff::FileDiff;
use crate::repo::{Error, Repo, git};

/// The identity checkpoint commits are written under.
///
/// Fixed on purpose: a checkpoint is bookkeeping, and a machine without `user.name` configured
/// must still capture.
const IDENTITY: [(&str, &str); 4] = [
    ("GIT_AUTHOR_NAME", "zdt"),
    ("GIT_AUTHOR_EMAIL", "zdt@checkpoint"),
    ("GIT_COMMITTER_NAME", "zdt"),
    ("GIT_COMMITTER_EMAIL", "zdt@checkpoint"),
];

/// The ref one turn's checkpoint is kept at.
#[must_use]
pub fn turn_ref(thread: i64, turn: i64, side: &str) -> String {
    format!("refs/zdt/checkpoints/{thread}/{turn}/{side}")
}

/// The prefix every one of a thread's checkpoints sits under.
#[must_use]
pub fn thread_prefix(thread: i64) -> String {
    format!("refs/zdt/checkpoints/{thread}/")
}

/// Captures the whole working tree as a hidden commit at `reference`.
///
/// Answers the commit's id. The capture goes through a temporary index file: read `HEAD`, add
/// everything, write the tree, commit it with no parent, point `reference` at it. The person's
/// index never learns any of it happened.
///
/// # Errors
///
/// When any of the steps refuses, which a broken repository does.
pub fn capture(repo: &Repo, reference: &str, message: &str) -> Result<String, Error> {
    let index = temp_index(repo);
    let spot = index.as_os_str();
    let env: &[(&str, &std::ffi::OsStr)] = &[("GIT_INDEX_FILE", spot)];

    let done = (|| {
        // A repository with no commits yet starts the capture from an empty tree.
        if git(repo, &["rev-parse", "--verify", "--quiet", "HEAD"], &[]).is_ok() {
            git(repo, &["read-tree", "HEAD"], env)?;
        } else {
            git(repo, &["read-tree", "--empty"], env)?;
        }
        git(repo, &["add", "-A"], env)?;
        let tree = git(repo, &["write-tree"], env)?;
        let tree = tree.trim();

        let identity: Vec<(&str, &std::ffi::OsStr)> = IDENTITY
            .iter()
            .map(|(name, value)| (*name, std::ffi::OsStr::new(*value)))
            .collect();
        let commit = git(repo, &["commit-tree", tree, "-m", message], &identity)?;
        let commit = commit.trim().to_owned();

        git(repo, &["update-ref", reference, &commit], &[])?;
        Ok(commit)
    })();

    let _ = std::fs::remove_file(&index);
    done
}

/// Puts the working tree back to what `reference` captured.
///
/// Tracked files come back through the index and the tree; files the capture never saw are
/// cleaned away. Ignored files are left alone.
///
/// # Errors
///
/// When the reference names nothing, or the tree cannot be written.
pub fn restore(repo: &Repo, reference: &str) -> Result<(), Error> {
    git(
        repo,
        &[
            "restore",
            "--source",
            reference,
            "--worktree",
            "--staged",
            "--",
            ".",
        ],
        &[],
    )?;
    git(repo, &["clean", "-fd"], &[])?;
    Ok(())
}

/// What changed between two checkpoints, file by file.
///
/// # Errors
///
/// When either reference cannot be read.
pub fn changes(repo: &Repo, old: &str, new: &str) -> Result<Vec<FileDiff>, Error> {
    crate::diff::commits(repo, old, new)
}

/// Removes every checkpoint ref under `prefix`, which deleting a thread does.
///
/// # Errors
///
/// When the refs cannot be listed.
pub fn forget(repo: &Repo, prefix: &str) -> Result<(), Error> {
    let listed = git(repo, &["for-each-ref", "--format=%(refname)", prefix], &[])?;
    for reference in listed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let _ = git(repo, &["update-ref", "-d", reference], &[]);
    }
    Ok(())
}

/// A path for the temporary index, beside the real one and never it.
fn temp_index(repo: &Repo) -> PathBuf {
    repo.dot_git().join(format!(
        "zdt-checkpoint-index-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ))
}

#[cfg(test)]
mod tests {
    use super::{capture, changes, forget, restore, turn_ref};
    use crate::repo::testing::Temp;

    #[test]
    fn a_capture_takes_untracked_files_and_leaves_the_index_alone() {
        let temp = Temp::new("checkpoint-capture");
        temp.commit("a.txt", "one\n", "first");
        temp.write("loose.txt", "untracked\n");

        let id =
            capture(&temp.repo(), &turn_ref(1, 1, "before"), "checkpoint").expect("it captures");
        assert_eq!(id.len(), 40);

        // The commit holds both files and has no parent.
        let held = temp.run(&["ls-tree", "--name-only", &id]);
        assert!(
            held.contains("a.txt") && held.contains("loose.txt"),
            "{held}"
        );
        assert!(
            temp.run(&["log", "--format=%p", "-1", &id])
                .trim()
                .is_empty(),
            "parentless"
        );
        // And nothing reached the person's index.
        assert!(
            temp.run(&["diff", "--cached", "--name-only"])
                .trim()
                .is_empty()
        );
    }

    #[test]
    fn a_restore_puts_edits_back_and_cleans_new_files_away() {
        let temp = Temp::new("checkpoint-restore");
        temp.commit("a.txt", "one\n", "first");
        let repo = temp.repo();
        let reference = turn_ref(1, 1, "before");
        capture(&repo, &reference, "before").expect("it captures");

        temp.write("a.txt", "changed\n");
        temp.write("made.txt", "new\n");
        restore(&repo, &reference).expect("it restores");

        assert_eq!(
            std::fs::read_to_string(temp.path("a.txt")).unwrap(),
            "one\n"
        );
        assert!(!temp.path("made.txt").exists(), "the new file is cleaned");
    }

    #[test]
    fn a_restore_leaves_ignored_files_alone() {
        let temp = Temp::new("checkpoint-ignored");
        temp.commit(".gitignore", "target/\n", "first");
        let repo = temp.repo();
        let reference = turn_ref(1, 1, "before");
        capture(&repo, &reference, "before").expect("it captures");

        temp.write("target/build.o", "artifact\n");
        restore(&repo, &reference).expect("it restores");
        assert!(temp.path("target/build.o").exists());
    }

    #[test]
    fn changes_between_two_captures_name_what_a_turn_did() {
        let temp = Temp::new("checkpoint-changes");
        temp.commit("a.txt", "one\n", "first");
        let repo = temp.repo();
        let before = turn_ref(1, 1, "before");
        let after = turn_ref(1, 1, "after");
        capture(&repo, &before, "before").expect("it captures");

        temp.write("a.txt", "one\ntwo\n");
        temp.write("b.txt", "fresh\n");
        capture(&repo, &after, "after").expect("it captures");

        let found = changes(&repo, &before, &after).expect("it diffs");
        let names: Vec<&str> = found.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(names, ["a.txt", "b.txt"]);
        assert_eq!(found[0].counts(), (1, 0));
        assert_eq!(found[1].counts(), (1, 0));
    }

    #[test]
    fn a_capture_works_in_a_repository_with_no_commits() {
        let temp = Temp::new("checkpoint-unborn");
        temp.write("a.txt", "one\n");
        let id = capture(&temp.repo(), &turn_ref(1, 1, "before"), "before").expect("it captures");
        assert!(temp.run(&["ls-tree", "--name-only", &id]).contains("a.txt"));
    }

    #[test]
    fn forgetting_a_thread_takes_its_refs_and_no_others() {
        let temp = Temp::new("checkpoint-forget");
        temp.commit("a.txt", "one\n", "first");
        let repo = temp.repo();
        capture(&repo, &turn_ref(1, 1, "before"), "x").expect("it captures");
        capture(&repo, &turn_ref(1, 1, "after"), "x").expect("it captures");
        capture(&repo, &turn_ref(2, 1, "before"), "x").expect("it captures");

        forget(&repo, &super::thread_prefix(1)).expect("it forgets");
        let left = temp.run(&["for-each-ref", "--format=%(refname)", "refs/zdt/"]);
        assert!(!left.contains("checkpoints/1/"), "{left}");
        assert!(left.contains("checkpoints/2/"), "{left}");
    }
}
