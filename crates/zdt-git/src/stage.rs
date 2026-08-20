//! Putting things into the index and taking them out again.
//!
//! # What staging one hunk actually is
//!
//! It is not a patch. What the index holds for a file is a blob. Staging part of a change means
//! the index must hold a *third* text: the one you get by applying the chosen hunks to the
//! committed text. It differs from what was committed and from what is on disk. So:
//!
//! 1. read the text the index currently holds;
//! 2. apply the chosen hunks to *that* text, in memory;
//! 3. write the result as a new blob;
//! 4. point the index entry at it, and write the index.
//!
//! Every step either happens or does not. Expressed as `git apply --cached` it is a patch that
//! has to be generated, escaped, and applied to a file that may have moved underneath it. When
//! that fails halfway, the index is left in a state nobody asked for.
//!
//! # Writing the index
//!
//! The whole index is rewritten each time, which is what git does too. It is one file, it is
//! usually small, and rewriting it whole makes a half-written index impossible.

use crate::diff::{DiffHunk, LineKind};
use crate::repo::{Error, Repo};

/// Puts everything about `path` into the index.
///
/// # Errors
///
/// When the file cannot be read or the index cannot be written.
pub fn stage_file(repo: &Repo, path: &str) -> Result<(), Error> {
    let full = repo.absolute(path);
    match std::fs::read(&full) {
        Ok(bytes) => write_entry(repo, path, Some(&bytes)),
        // Gone from the working tree: staging it stages the removal, which is what `git add` on a
        // deleted file does.
        Err(_) => write_entry(repo, path, None),
    }
}

/// Takes everything about `path` back out of the index.
///
/// Back to what the last commit holds. When the last commit has never heard of the file, it goes
/// out of the index altogether, which is what unstaging a newly added file means.
///
/// # Errors
///
/// When the index cannot be written.
pub fn unstage_file(repo: &Repo, path: &str) -> Result<(), Error> {
    let committed = crate::diff::head_blob(repo, path)?;
    write_entry(repo, path, committed.as_deref())
}

/// Takes `path`, and everything under it, out of the index and leaves it on disk.
///
/// What `git rm --cached` does. The file stays where it is and becomes untracked, and its removal
/// is what the next commit records.
///
/// One signature for a file and for a directory, because the tree acts on rows and a row is
/// either.
///
/// Different from [`unstage_file`], which puts back what the last commit holds. This says the file
/// should stop being tracked at all.
///
/// # Errors
///
/// When the index cannot be written.
pub fn untrack(repo: &Repo, path: &str) -> Result<(), Error> {
    let mut index = repo.git().open_index().map_err(Error::git)?;
    let name = path.as_bytes().to_owned();
    let under = format!("{path}/").into_bytes();
    index.remove_entries(|_, entry_path, _| {
        entry_path == name.as_slice() || entry_path.starts_with(under.as_slice())
    });
    index
        .write(gix::index::write::Options::default())
        .map_err(Error::git)?;
    Ok(())
}

/// Puts just these hunks of `path` into the index.
///
/// The hunks come from the *unstaged* diff, which is the working tree against the index. Applying
/// them to what the index holds produces the text that should be staged.
///
/// # Errors
///
/// When the file cannot be read, a hunk does not fit the text it names, or the index cannot be
/// written.
pub fn stage_hunks(repo: &Repo, path: &str, hunks: &[DiffHunk]) -> Result<(), Error> {
    if hunks.is_empty() {
        return Ok(());
    }
    let base = crate::diff::index_blob(repo, path)?.unwrap_or_default();
    let staged = apply(&base, hunks, false)?;
    write_entry(repo, path, Some(&staged))
}

/// Takes just these hunks of `path` back out of the index.
///
/// The hunks come from the *staged* diff, and are applied backwards: what they added is taken out
/// and what they removed is put back.
///
/// # Errors
///
/// As [`stage_hunks`].
pub fn unstage_hunks(repo: &Repo, path: &str, hunks: &[DiffHunk]) -> Result<(), Error> {
    if hunks.is_empty() {
        return Ok(());
    }
    let base = crate::diff::index_blob(repo, path)?.unwrap_or_default();
    let staged = apply(&base, hunks, true)?;
    write_entry(repo, path, Some(&staged))
}

/// Throws away what is not staged in `path`, putting back what the index holds.
///
/// # Errors
///
/// When the file cannot be written.
pub fn discard_file(repo: &Repo, path: &str) -> Result<(), Error> {
    let full = repo.absolute(path);
    match crate::diff::index_blob(repo, path)? {
        Some(bytes) => {
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| Error::Git(format!("{}: {error}", parent.display())))?;
            }
            std::fs::write(&full, bytes)
                .map_err(|error| Error::Git(format!("{}: {error}", full.display())))
        }
        // The index has never heard of it, so throwing away the change means the file itself.
        None => match std::fs::remove_file(&full) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(Error::Git(format!("{}: {error}", full.display()))),
        },
    }
}

/// Throws away just these hunks of `path`.
///
/// # Errors
///
/// As [`discard_file`].
pub fn discard_hunks(repo: &Repo, path: &str, hunks: &[DiffHunk]) -> Result<(), Error> {
    if hunks.is_empty() {
        return Ok(());
    }
    let full = repo.absolute(path);
    let current = std::fs::read(&full).unwrap_or_default();
    // Backwards, because the hunks say what the working tree has that the index does not, and
    // throwing them away means undoing exactly that.
    let put_back = apply(&current, hunks, true)?;
    std::fs::write(&full, put_back)
        .map_err(|error| Error::Git(format!("{}: {error}", full.display())))
}

/// Applies `hunks` to `base`, or undoes them when `backwards`.
///
/// Works in lines, matching each hunk against the text by its context. A hunk whose context does
/// not match is refused. A hunk applied at a guess corrupts the file quietly, and the file on disk
/// is the only copy of somebody's work.
fn apply(base: &[u8], hunks: &[DiffHunk], backwards: bool) -> Result<Vec<u8>, Error> {
    let ends_with_newline = base.is_empty() || base.ends_with(b"\n");
    let text = String::from_utf8_lossy(base).into_owned();
    let mut lines: Vec<String> = if text.is_empty() {
        Vec::new()
    } else {
        let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
        if text.ends_with('\n') {
            lines.pop();
        }
        lines
    };

    // Back to front, so that each hunk's line numbers still refer to where they were: applying one
    // hunk moves everything after it.
    let mut ordered: Vec<&DiffHunk> = hunks.iter().collect();
    ordered.sort_by_key(|hunk| std::cmp::Reverse(start_of(hunk, backwards)));

    for hunk in ordered {
        // What the hunk expects to find, and what it puts there instead. Read backwards, the two
        // swap: undoing a change means finding what it produced and putting back what it replaced.
        let (expected, replacement) = if backwards {
            (kept(hunk, LineKind::Added), kept(hunk, LineKind::Removed))
        } else {
            (kept(hunk, LineKind::Removed), kept(hunk, LineKind::Added))
        };

        let at = locate(&lines, &expected, start_of(hunk, backwards)).ok_or_else(|| {
            Error::Git(format!(
                "the change at line {} is not where it says it is",
                start_of(hunk, backwards) + 1
            ))
        })?;

        lines.splice(at..at + expected.len(), replacement);
    }

    let mut out = lines.join("\n");
    if ends_with_newline && !out.is_empty() {
        out.push('\n');
    }
    Ok(out.into_bytes())
}

/// The lines of a hunk that are of one kind, plus the context. This is what has to be found.
fn kept(hunk: &DiffHunk, changed: LineKind) -> Vec<String> {
    hunk.lines
        .iter()
        .filter(|line| line.kind == LineKind::Context || line.kind == changed)
        .map(|line| line.text.clone())
        .collect()
}

/// Where a hunk starts, counting from zero.
fn start_of(hunk: &DiffHunk, backwards: bool) -> usize {
    let one_based = if backwards {
        hunk.new_start
    } else {
        hunk.old_start
    };
    one_based.saturating_sub(1) as usize
}

/// Where `wanted` is in `lines`, looking near `hint` first.
///
/// The hint is where the hunk says it is. The search widens from there, because a file may have
/// been edited above the hunk since the diff was taken. When the hinted place fits, that is the
/// match. A hunk that fits where it says it does belongs there.
fn locate(lines: &[String], wanted: &[String], hint: usize) -> Option<usize> {
    if wanted.is_empty() {
        return (hint <= lines.len()).then_some(hint);
    }
    let fits =
        |at: usize| at + wanted.len() <= lines.len() && lines[at..at + wanted.len()] == *wanted;

    if fits(hint) {
        return Some(hint);
    }
    // Outwards from the hint, nearest first, so the match found is the one meant.
    for distance in 1..=lines.len() {
        if let Some(before) = hint.checked_sub(distance)
            && fits(before)
        {
            return Some(before);
        }
        let after = hint + distance;
        if after >= lines.len() {
            if hint < distance {
                break;
            }
            continue;
        }
        if fits(after) {
            return Some(after);
        }
    }
    None
}

/// Points the index entry for `path` at `content`, or takes it out when there is none.
fn write_entry(repo: &Repo, path: &str, content: Option<&[u8]>) -> Result<(), Error> {
    use gix::index::entry::{Flags, Mode, Stat};

    let git = repo.git();
    // The file itself, and not the shared snapshot, because this is about to write it.
    let mut index = git.open_index().map_err(Error::git)?;
    let name: &gix::bstr::BStr = path.into();

    match content {
        None => {
            index.remove_entries(|_, entry_path, _| entry_path == name);
        }
        Some(bytes) => {
            let id = git.write_blob(bytes).map_err(Error::git)?.detach();
            // The mode the file already has, so staging a change does not silently make an
            // executable file ordinary.
            let mode = executable(repo, path).map_or(Mode::FILE, |yes| {
                if yes {
                    Mode::FILE_EXECUTABLE
                } else {
                    Mode::FILE
                }
            });
            match index.entry_index_by_path(name) {
                Ok(at) => {
                    let entry = &mut index.entries_mut()[at];
                    entry.id = id;
                    entry.mode = mode;
                    // The stat is what makes git believe the working tree matches the index
                    // without reading every file. Zeroed, because git trusts it and a wrong stat
                    // is worse than none.
                    entry.stat = Stat::default();
                }
                Err(_) => {
                    index.dangerously_push_entry(Stat::default(), id, Flags::empty(), mode, name);
                    index.sort_entries();
                }
            }
        }
    }

    index
        .write(gix::index::write::Options::default())
        .map_err(Error::git)?;
    Ok(())
}

/// Whether the file on disk has its executable bit set.
fn executable(repo: &Repo, path: &str) -> Option<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(repo.absolute(path)).ok()?;
        Some(metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        let _ = (repo, path);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{
        discard_file, discard_hunks, stage_file, stage_hunks, unstage_file, unstage_hunks, untrack,
    };
    use crate::diff;
    use crate::repo::testing::Temp;

    #[test]
    fn staging_a_file_puts_all_of_it_in_the_index() {
        let temp = Temp::new("stage-file");
        temp.commit("a.txt", "one\ntwo\n", "first");
        temp.write("a.txt", "one\nTWO\n");

        stage_file(&temp.repo(), "a.txt").expect("it stages");

        // Asserted against git itself, because what is being tested is whether git agrees.
        assert!(
            temp.run(&["diff", "--cached", "--name-only"])
                .contains("a.txt"),
            "git sees it staged"
        );
        assert!(
            temp.run(&["diff", "--name-only"]).trim().is_empty(),
            "and nothing left unstaged"
        );
    }

    #[test]
    fn unstaging_a_file_takes_all_of_it_back_out() {
        let temp = Temp::new("unstage-file");
        temp.commit("a.txt", "one\ntwo\n", "first");
        temp.write("a.txt", "one\nTWO\n");
        temp.run(&["add", "a.txt"]);

        unstage_file(&temp.repo(), "a.txt").expect("it unstages");

        assert!(
            temp.run(&["diff", "--cached", "--name-only"])
                .trim()
                .is_empty(),
            "nothing is staged"
        );
        assert!(
            temp.run(&["diff", "--name-only"]).contains("a.txt"),
            "and the change is still in the working tree"
        );
    }

    #[test]
    fn unstaging_a_new_file_leaves_it_untracked() {
        let temp = Temp::new("unstage-new");
        temp.commit("a.txt", "one\n", "first");
        temp.write("new.txt", "fresh\n");
        temp.run(&["add", "new.txt"]);

        unstage_file(&temp.repo(), "new.txt").expect("it unstages");

        assert!(
            temp.run(&["status", "--porcelain"]).contains("?? new.txt"),
            "git calls it untracked: {}",
            temp.run(&["status", "--porcelain"])
        );
    }

    #[test]
    fn untracking_a_file_leaves_it_on_disk() {
        let temp = Temp::new("untrack-file");
        temp.commit("a.txt", "one\n", "first");

        untrack(&temp.repo(), "a.txt").expect("it untracks");

        assert!(temp.path("a.txt").exists(), "the file is still there");
        let status = temp.run(&["status", "--porcelain"]);
        assert!(
            status.contains("D  a.txt") && status.contains("?? a.txt"),
            "git records the removal and calls the file untracked: {status}"
        );
    }

    #[test]
    fn untracking_a_directory_takes_everything_under_it() {
        let temp = Temp::new("untrack-directory");
        temp.commit("keep.txt", "one\n", "first");
        std::fs::create_dir_all(temp.path("out")).expect("made");
        temp.write("out/a.txt", "one\n");
        temp.write("out/b.txt", "two\n");
        temp.run(&["add", "out"]);
        temp.run(&["commit", "-m", "second"]);

        untrack(&temp.repo(), "out").expect("it untracks");

        let tracked = temp.run(&["ls-files"]);
        assert!(
            !tracked.contains("out/"),
            "nothing under it is left: {tracked}"
        );
        assert!(
            tracked.contains("keep.txt"),
            "and its neighbour is untouched"
        );
    }

    #[test]
    fn untracking_leaves_a_neighbour_with_the_same_prefix_alone() {
        // `out` and `outside` share a prefix, and only the directory was named.
        let temp = Temp::new("untrack-prefix");
        std::fs::create_dir_all(temp.path("out")).expect("made");
        std::fs::create_dir_all(temp.path("outside")).expect("made");
        temp.write("out/a.txt", "one\n");
        temp.write("outside/b.txt", "two\n");
        temp.run(&["add", "."]);
        temp.run(&["commit", "-m", "first"]);

        untrack(&temp.repo(), "out").expect("it untracks");

        let tracked = temp.run(&["ls-files"]);
        assert!(!tracked.contains("out/a.txt"));
        assert!(tracked.contains("outside/b.txt"), "{tracked}");
    }

    #[test]
    fn staging_one_hunk_stages_exactly_that_hunk() {
        // The whole reason this crate uses gix. Two changes far enough apart to be two hunks,
        // and only the first is staged.
        let temp = Temp::new("stage-hunk");
        let original: String = (0..40).map(|n| format!("line {n}\n")).collect();
        temp.commit("a.txt", &original, "first");

        let mut lines: Vec<String> = (0..40).map(|n| format!("line {n}")).collect();
        lines[2] = "changed near the top".to_owned();
        lines[35] = "changed near the bottom".to_owned();
        temp.write(
            "a.txt",
            &lines.iter().map(|l| format!("{l}\n")).collect::<String>(),
        );

        let repo = temp.repo();
        let found = diff::worktree(&repo, "a.txt").expect("it diffs");
        assert_eq!(found.hunks.len(), 2, "two hunks to choose between");

        stage_hunks(&repo, "a.txt", &found.hunks[..1]).expect("it stages one");

        let staged = temp.run(&["diff", "--cached"]);
        assert!(
            staged.contains("changed near the top"),
            "the first went in:\n{staged}"
        );
        assert!(
            !staged.contains("changed near the bottom"),
            "and the second did not:\n{staged}"
        );

        let unstaged = temp.run(&["diff"]);
        assert!(
            unstaged.contains("changed near the bottom"),
            "which is still waiting in the working tree:\n{unstaged}"
        );
        assert!(!unstaged.contains("changed near the top"));
    }

    #[test]
    fn unstaging_one_hunk_takes_exactly_that_hunk_back() {
        let temp = Temp::new("unstage-hunk");
        let original: String = (0..40).map(|n| format!("line {n}\n")).collect();
        temp.commit("a.txt", &original, "first");

        let mut lines: Vec<String> = (0..40).map(|n| format!("line {n}")).collect();
        lines[2] = "top".to_owned();
        lines[35] = "bottom".to_owned();
        temp.write(
            "a.txt",
            &lines.iter().map(|l| format!("{l}\n")).collect::<String>(),
        );
        temp.run(&["add", "a.txt"]);

        let repo = temp.repo();
        let staged = diff::staged(&repo, "a.txt").expect("it diffs");
        assert_eq!(staged.hunks.len(), 2);

        unstage_hunks(&repo, "a.txt", &staged.hunks[..1]).expect("it unstages one");

        let still = temp.run(&["diff", "--cached"]);
        assert!(!still.contains("top"), "the first came back out:\n{still}");
        assert!(
            still.contains("bottom"),
            "and the second stayed in:\n{still}"
        );
    }

    #[test]
    fn discarding_a_file_puts_the_index_version_back() {
        let temp = Temp::new("discard-file");
        temp.commit("a.txt", "one\ntwo\n", "first");
        temp.write("a.txt", "one\nRUINED\n");

        discard_file(&temp.repo(), "a.txt").expect("it discards");
        assert_eq!(
            std::fs::read_to_string(temp.path("a.txt")).expect("it reads"),
            "one\ntwo\n"
        );
    }

    #[test]
    fn discarding_a_file_the_index_never_had_removes_it() {
        let temp = Temp::new("discard-new");
        temp.commit("a.txt", "one\n", "first");
        temp.write("new.txt", "fresh\n");

        discard_file(&temp.repo(), "new.txt").expect("it discards");
        assert!(!temp.path("new.txt").exists());
    }

    #[test]
    fn discarding_one_hunk_leaves_the_other_alone() {
        let temp = Temp::new("discard-hunk");
        let original: String = (0..40).map(|n| format!("line {n}\n")).collect();
        temp.commit("a.txt", &original, "first");

        let mut lines: Vec<String> = (0..40).map(|n| format!("line {n}")).collect();
        lines[2] = "keep me".to_owned();
        lines[35] = "throw me away".to_owned();
        temp.write(
            "a.txt",
            &lines.iter().map(|l| format!("{l}\n")).collect::<String>(),
        );

        let repo = temp.repo();
        let found = diff::worktree(&repo, "a.txt").expect("it diffs");
        discard_hunks(&repo, "a.txt", &found.hunks[1..]).expect("it discards the second");

        let now = std::fs::read_to_string(temp.path("a.txt")).expect("it reads");
        assert!(now.contains("keep me"), "the first is still there:\n{now}");
        assert!(!now.contains("throw me away"), "and the second is gone");
        assert!(now.contains("line 35"), "which put the original line back");
    }

    #[test]
    fn staging_a_deleted_file_stages_the_deletion() {
        let temp = Temp::new("stage-delete");
        temp.commit("a.txt", "one\n", "first");
        std::fs::remove_file(temp.path("a.txt")).expect("it goes");

        stage_file(&temp.repo(), "a.txt").expect("it stages");
        assert!(
            temp.run(&["diff", "--cached", "--name-status"])
                .starts_with('D'),
            "git sees a deletion: {}",
            temp.run(&["diff", "--cached", "--name-status"])
        );
    }

    #[test]
    fn a_file_with_no_trailing_newline_keeps_not_having_one() {
        // Otherwise staging a hunk would quietly add a line to every such file, and the diff
        // afterwards would show a change nobody made.
        let temp = Temp::new("stage-newline");
        temp.commit("a.txt", "one\ntwo", "first");
        temp.write("a.txt", "one\nTWO");

        let repo = temp.repo();
        let found = diff::worktree(&repo, "a.txt").expect("it diffs");
        stage_hunks(&repo, "a.txt", &found.hunks).expect("it stages");

        let staged = crate::diff::index_blob(&repo, "a.txt")
            .expect("it reads")
            .expect("it is there");
        assert_eq!(String::from_utf8_lossy(&staged), "one\nTWO");
    }

    #[test]
    fn staging_every_hunk_is_the_same_as_staging_the_file() {
        let temp = Temp::new("stage-all-hunks");
        let original: String = (0..40).map(|n| format!("line {n}\n")).collect();
        temp.commit("a.txt", &original, "first");

        let mut lines: Vec<String> = (0..40).map(|n| format!("line {n}")).collect();
        lines[2] = "one".to_owned();
        lines[35] = "two".to_owned();
        let changed: String = lines.iter().map(|l| format!("{l}\n")).collect();
        temp.write("a.txt", &changed);

        let repo = temp.repo();
        let found = diff::worktree(&repo, "a.txt").expect("it diffs");
        stage_hunks(&repo, "a.txt", &found.hunks).expect("it stages every one");

        let staged = crate::diff::index_blob(&repo, "a.txt")
            .expect("it reads")
            .expect("it is there");
        assert_eq!(String::from_utf8_lossy(&staged), changed);
        assert!(
            temp.run(&["diff", "--name-only"]).trim().is_empty(),
            "nothing is left unstaged"
        );
    }
}
