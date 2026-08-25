//! Making a commit.
//!
//! # The one thing that is not gix
//!
//! Everything else in this crate goes through the object store. This module runs `git commit`,
//! because `gix` 0.85 has no way to write a tree from an index. `gix-index` reads and writes the
//! index file and stops there, and without index-to-tree there is no commit to make.
//!
//! The alternative was to build the tree here: sort the index entries, group them into nested
//! trees, write each one, and match git's ordering rules exactly. Getting that subtly wrong
//! produces a repository that looks fine until somebody clones it. `git commit` gets it right by
//! definition. It is one process at the one moment when a person has stopped to type a message,
//! which is the one moment in this crate where a process spawn costs nothing anybody can feel.
//!
//! No shell is involved. The message is an argument, so quoting, backticks and newlines in it are
//! ordinary characters.

use crate::repo::{Error, Repo};

/// Commits whatever is in the index, with `message`.
///
/// Answers the new commit's identifier.
///
/// `amend` replaces the last commit and keeps its parents. That is what somebody who has just
/// noticed a typo in their message wants.
///
/// # Errors
///
/// When the message is empty, when there is nothing staged, or when git refuses. An empty message
/// is refused here. Git refuses it too, and refusing early lets the panel say so with no process
/// spawn.
pub fn commit(repo: &Repo, message: &str, amend: bool) -> Result<String, Error> {
    let message = message.trim();
    if message.is_empty() {
        return Err(Error::Git("a commit needs a message".to_owned()));
    }

    let mut args: Vec<&str> = vec!["commit", "--quiet", "--no-verify", "-m", message];
    if amend {
        args.push("--amend");
    }
    crate::repo::git(repo, &args, &[])?;

    // What it turned into, read back through the object store like everything else.
    crate::log::find(repo, "HEAD").map(|commit| commit.id)
}

/// Stages everything in the working tree and commits it, with `message`.
///
/// What an agent thread's "commit" button means: the whole tree as it stands, one commit.
///
/// # Errors
///
/// As [`commit`], and when staging fails.
pub fn commit_all(repo: &Repo, message: &str) -> Result<String, Error> {
    crate::repo::git(repo, &["add", "-A"], &[])?;
    commit(repo, message, false)
}

/// Stages and commits only `paths`, with `message`.
///
/// What the commit modal's file checkboxes mean. `git commit -- <paths>` builds the commit from
/// the working state of those paths alone, so whatever else stands in the index stays out of it
/// and stays staged.
///
/// # Errors
///
/// As [`commit`], and when staging fails.
pub fn commit_paths(repo: &Repo, message: &str, paths: &[String]) -> Result<String, Error> {
    let message = message.trim();
    if message.is_empty() {
        return Err(Error::Git("a commit needs a message".to_owned()));
    }
    if paths.is_empty() {
        return Err(Error::Git(
            "a commit of chosen files needs files".to_owned(),
        ));
    }
    // Staged first so an untracked file among the paths is a file git will take.
    let mut add: Vec<&str> = vec!["add", "-A", "--"];
    add.extend(paths.iter().map(String::as_str));
    crate::repo::git(repo, &add, &[])?;

    let mut args: Vec<&str> = vec!["commit", "--quiet", "--no-verify", "-m", message, "--"];
    args.extend(paths.iter().map(String::as_str));
    crate::repo::git(repo, &args, &[])?;
    crate::log::find(repo, "HEAD").map(|commit| commit.id)
}

/// Makes a branch at `HEAD` and moves the checkout onto it.
///
/// What "commit to a new branch" starts with. The working tree is untouched.
///
/// # Errors
///
/// When a branch of that name exists, or git refuses.
pub fn switch_new(repo: &Repo, branch: &str) -> Result<(), Error> {
    crate::repo::git(repo, &["switch", "--quiet", "-c", branch], &[])?;
    Ok(())
}

/// One file a commit of the whole tree would touch.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PendingFile {
    /// Its path, relative to the repository root.
    pub path: String,
    /// Lines added.
    pub added: u32,
    /// Lines taken away.
    pub removed: u32,
    /// Whether the counts mean nothing because the file is binary.
    pub binary: bool,
}

/// Everything a commit of the whole tree would take: the files with their counts, and the patch.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Pending {
    /// The files, in git's order.
    pub files: Vec<PendingFile>,
    /// The whole patch, as `git diff` writes it.
    pub patch: String,
}

/// What committing the whole tree would commit, untracked files included.
///
/// Staged through a temporary index, so the real one is untouched — the same trick the
/// checkpoints use.
///
/// # Errors
///
/// When git refuses.
pub fn pending(repo: &Repo) -> Result<Pending, Error> {
    let index = repo.dot_git().join(format!(
        "zdt-pending-index-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let spot = index.as_os_str();
    let env: &[(&str, &std::ffi::OsStr)] = &[("GIT_INDEX_FILE", spot)];

    let done = (|| {
        // A repository with no commits yet compares against an empty tree.
        if crate::repo::git(repo, &["rev-parse", "--verify", "--quiet", "HEAD"], &[]).is_ok() {
            crate::repo::git(repo, &["read-tree", "HEAD"], env)?;
        } else {
            crate::repo::git(repo, &["read-tree", "--empty"], env)?;
        }
        crate::repo::git(repo, &["add", "-A"], env)?;

        let numbers = crate::repo::git(repo, &["diff", "--cached", "--numstat"], env)?;
        let files = numbers
            .lines()
            .filter_map(|line| {
                let mut parts = line.splitn(3, '\t');
                let added = parts.next()?;
                let removed = parts.next()?;
                let path = parts.next()?.to_owned();
                let binary = added == "-";
                Some(PendingFile {
                    path,
                    added: added.parse().unwrap_or(0),
                    removed: removed.parse().unwrap_or(0),
                    binary,
                })
            })
            .collect();
        let patch = crate::repo::git(repo, &["diff", "--cached"], env)?;
        Ok(Pending { files, patch })
    })();
    let _ = std::fs::remove_file(&index);
    done
}

/// Pushes the current branch to `origin`, setting the upstream when there is none yet.
///
/// # Errors
///
/// When there is no remote called `origin`, or the network says no.
pub fn push(repo: &Repo) -> Result<(), Error> {
    crate::repo::git(repo, &["push", "--quiet", "-u", "origin", "HEAD"], &[])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{commit, pending, switch_new};
    use crate::repo::testing::Temp;

    #[test]
    fn pending_counts_edits_and_untracked_files_and_leaves_the_index_alone() {
        let temp = Temp::new("commit-pending");
        temp.commit("a.txt", "one\n", "first");
        temp.write("a.txt", "one\ntwo\nthree\n");
        temp.write("b.txt", "fresh\n");

        let found = pending(&temp.repo()).expect("it scans");
        assert_eq!(found.files.len(), 2);
        let a = found
            .files
            .iter()
            .find(|file| file.path == "a.txt")
            .unwrap();
        assert_eq!((a.added, a.removed), (2, 0));
        let b = found
            .files
            .iter()
            .find(|file| file.path == "b.txt")
            .unwrap();
        assert_eq!((b.added, b.removed), (1, 0));
        assert!(found.patch.contains("+fresh"), "the patch has the new file");
        assert!(
            temp.run(&["diff", "--cached", "--numstat"])
                .trim()
                .is_empty(),
            "the real index never learned any of it"
        );
    }

    #[test]
    fn committing_chosen_paths_leaves_the_rest_dirty() {
        let temp = Temp::new("commit-paths");
        temp.commit("a.txt", "one\n", "first");
        temp.write("a.txt", "one\ntwo\n");
        temp.write("kept.txt", "out of the commit\n");
        temp.write("taken.txt", "in the commit\n");

        let paths = vec!["a.txt".to_owned(), "taken.txt".to_owned()];
        super::commit_paths(&temp.repo(), "chosen", &paths).expect("it commits");

        assert_eq!(temp.run(&["log", "-1", "--format=%s"]).trim(), "chosen");
        let shown = temp.run(&["show", "--stat", "--format=", "HEAD"]);
        assert!(shown.contains("a.txt") && shown.contains("taken.txt"));
        assert!(!shown.contains("kept.txt"), "the unchosen file stayed out");
        assert!(
            temp.run(&["status", "--porcelain"]).contains("kept.txt"),
            "and it is still there to commit later"
        );
    }

    #[test]
    fn switch_new_moves_the_checkout_onto_a_fresh_branch() {
        let temp = Temp::new("commit-switch");
        temp.commit("a.txt", "one\n", "first");

        switch_new(&temp.repo(), "feature/fresh").expect("it switches");
        assert_eq!(
            temp.run(&["branch", "--show-current"]).trim(),
            "feature/fresh"
        );
        assert!(switch_new(&temp.repo(), "feature/fresh").is_err());
    }

    #[test]
    fn what_is_staged_is_what_is_committed() {
        let temp = Temp::new("commit-basic");
        temp.commit("a.txt", "one\n", "first");
        temp.write("a.txt", "one\ntwo\n");
        temp.run(&["add", "a.txt"]);

        let id = commit(&temp.repo(), "second", false).expect("it commits");
        assert_eq!(id.len(), 40);

        assert_eq!(temp.run(&["log", "-1", "--format=%s"]).trim(), "second");
        assert_eq!(
            temp.run(&["rev-parse", "HEAD"]).trim(),
            id,
            "and it says which commit it made"
        );
        assert!(
            temp.run(&["status", "--porcelain"]).trim().is_empty(),
            "with nothing left over"
        );
    }

    #[test]
    fn the_first_commit_needs_no_parent() {
        let temp = Temp::new("commit-first");
        temp.write("a.txt", "one\n");
        temp.run(&["add", "a.txt"]);

        commit(&temp.repo(), "first", false).expect("it commits");
        assert_eq!(temp.run(&["log", "--format=%s"]).trim(), "first");
    }

    #[test]
    fn a_message_of_nothing_is_refused() {
        // A commit with no message is one nobody can find again.
        let temp = Temp::new("commit-nomessage");
        temp.write("a.txt", "one\n");
        temp.run(&["add", "a.txt"]);

        assert!(commit(&temp.repo(), "   \n  ", false).is_err());
        assert!(commit(&temp.repo(), "", false).is_err());
        // And nothing happened. This counts the commits, because `git log` in a repository with
        // no commits in it is itself an error.
        assert_eq!(temp.run(&["rev-list", "--count", "--all"]).trim(), "0");
    }

    #[test]
    fn committing_nothing_is_refused() {
        // Almost always somebody who thought they had staged something.
        let temp = Temp::new("commit-empty");
        temp.commit("a.txt", "one\n", "first");

        assert!(commit(&temp.repo(), "second", false).is_err());
        assert_eq!(
            temp.run(&["rev-list", "--count", "HEAD"]).trim(),
            "1",
            "no commit was made"
        );
    }

    #[test]
    fn amending_replaces_the_last_commit_rather_than_adding_one() {
        let temp = Temp::new("commit-amend");
        temp.commit("a.txt", "one\n", "first");
        temp.commit("b.txt", "two\n", "the typo");

        commit(&temp.repo(), "the fix", true).expect("it amends");

        assert_eq!(temp.run(&["rev-list", "--count", "HEAD"]).trim(), "2");
        assert_eq!(temp.run(&["log", "-1", "--format=%s"]).trim(), "the fix");
        assert!(
            temp.run(&["log", "--format=%s"]).contains("first"),
            "and the history under it is untouched"
        );
    }

    #[test]
    fn a_multi_line_message_keeps_its_body() {
        let temp = Temp::new("commit-body");
        temp.write("a.txt", "one\n");
        temp.run(&["add", "a.txt"]);

        commit(&temp.repo(), "the summary\n\nthe body.", false).expect("it commits");
        assert_eq!(
            temp.run(&["log", "-1", "--format=%s"]).trim(),
            "the summary"
        );
        assert_eq!(temp.run(&["log", "-1", "--format=%b"]).trim(), "the body.");
    }

    #[test]
    fn a_message_full_of_shell_is_a_message() {
        // No shell is involved, so none of this means anything: it is one argument.
        let temp = Temp::new("commit-shell");
        temp.write("a.txt", "one\n");
        temp.run(&["add", "a.txt"]);

        let awkward = "fix `rm -rf $HOME`; \"quoted\" & $(subshell)";
        commit(&temp.repo(), awkward, false).expect("it commits");
        assert_eq!(temp.run(&["log", "-1", "--format=%s"]).trim(), awkward);
        assert!(temp.path("a.txt").exists(), "and nothing was run");
    }
}
