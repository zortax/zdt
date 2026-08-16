use super::text::{is_binary, lines_of};
use super::{DiffHunk, LineKind, between, staged, worktree};
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
    // Four lines, the third changed. Git says `@@ -2,3 +2,3 @@`, which is three lines of context
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
    // Which is every file in it added. A missing parent is an empty tree, and not an error.
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
