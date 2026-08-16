//! Making a commit.
//!
//! # The one thing that is not gix
//!
//! Everything else in this crate goes through the object store. This does not, because `gix` 0.85
//! has no way to write a tree from an index — `gix-index` reads and writes the index file and
//! stops there, and without index-to-tree there is no commit to make.
//!
//! The alternative was to build the tree here: sort the index entries, group them into nested
//! trees, write each one, and hope the ordering rules match git's exactly. Getting that subtly
//! wrong produces a repository that looks fine until somebody clones it. Running `git commit` gets
//! it exactly right by definition, and it is one process at the one moment when a person has
//! stopped to type a message — which is the one moment in this whole crate where a process spawn
//! costs nothing anybody can perceive.
//!
//! No shell is involved: the message is an argument, so quoting, backticks and newlines in it are
//! ordinary characters rather than something to escape.

use std::process::Command;

use crate::repo::{Error, Repo};

/// Commits whatever is in the index, with `message`.
///
/// Answers the new commit's identifier.
///
/// `amend` replaces the last commit rather than adding one, keeping its parents — which is what
/// somebody who has just noticed a typo in their message wants.
///
/// # Errors
///
/// When the message is empty, when there is nothing staged, or when git refuses. An empty message
/// is refused here rather than passed on: git refuses it too, and refusing early means the panel
/// says so without a process spawn.
pub fn commit(repo: &Repo, message: &str, amend: bool) -> Result<String, Error> {
    let message = message.trim();
    if message.is_empty() {
        return Err(Error::Git("a commit needs a message".to_owned()));
    }

    let mut args: Vec<&str> = vec!["commit", "--quiet", "--no-verify", "-m", message];
    if amend {
        args.push("--amend");
    }
    run(repo, &args)?;

    // What it turned into, read back through the object store like everything else.
    crate::log::find(repo, "HEAD").map(|commit| commit.id)
}

/// Runs git in the repository, or says what it said.
fn run(repo: &Repo, args: &[&str]) -> Result<String, Error> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo.root())
        .args(args)
        .output()
        .map_err(|error| Error::Git(format!("git could not be run: {error}")))?;

    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }

    // What git printed, as one line. Its first line is the useful one; the rest is advice about
    // command-line flags this panel does not have.
    let said = String::from_utf8_lossy(&output.stderr);
    let first = said
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("git refused");
    Err(Error::Git(first.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::commit;
    use crate::repo::testing::Temp;

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
        // And nothing happened. Counted rather than logged, because `git log` in a repository
        // with no commits in it is itself an error.
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
