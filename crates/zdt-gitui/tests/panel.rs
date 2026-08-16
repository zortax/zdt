//! The git panel, driven the way a person drives it.
//!
//! The repository operations themselves are asserted in `zdt-git`, against real repositories and
//! against what `git` itself says. What is asserted here is the layer above them: the panel reads
//! what the repository holds, the caret moves between its lists, and `s` on a file is the file git
//! then reports as staged.
//!
//! Every one of these builds a real repository by running `git`. A fixture built with the same
//! library under test could agree with it and both be wrong.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

use zdt_gitui::{GitUi, List, Nowhere, Selected, View};
use zgui_testkit_view::Window;

/// A repository that removes itself.
struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "zdt-gitui-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a directory");

        let temp = Self(root);
        temp.run(&["init", "--initial-branch=main"]);
        temp.run(&["config", "user.email", "test@example.com"]);
        temp.run(&["config", "user.name", "Test"]);
        temp.run(&["config", "commit.gpgsign", "false"]);
        temp
    }

    fn run(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.0)
            .args(args)
            .output()
            .unwrap_or_else(|error| panic!("git {args:?}: {error}"));
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn write(&self, name: &str, text: &str) {
        std::fs::write(self.0.join(name), text).expect("a file");
    }

    fn commit(&self, name: &str, text: &str, message: &str) {
        self.write(name, text);
        self.run(&["add", name]);
        self.run(&["commit", "-m", message]);
    }

    fn root(&self) -> &Path {
        &self.0
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A panel over `temp`, and the window keeping its reactive scope alive.
///
/// The panel reads on a worker, so every assertion has to let the work finish. `settle` does that
/// by advancing both clocks.
struct Panel {
    window: Window,
    git: GitUi,
}

impl Panel {
    fn open(temp: &Temp) -> Self {
        let window = Window::open();
        let git = window
            .scope
            .with(|| GitUi::new(temp.root(), Rc::new(Nowhere)));
        let panel = Self { window, git };
        panel.git.refresh();
        panel.settle();
        panel
    }

    /// Lets whatever was started finish.
    ///
    /// Both clocks: the harness's, which the timers run on, and the wall clock, which the worker
    /// threads run on. Advancing one without the other waits for nothing.
    fn settle(&self) {
        for _ in 0..80 {
            self.window.advance(std::time::Duration::from_millis(5));
            self.window.frame();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    /// Runs `body` inside the panel's scope, which is what its tasks need.
    fn with<R>(&self, body: impl FnOnce(&GitUi) -> R) -> R {
        let git = self.git.clone();
        self.window.scope.with(|| body(&git))
    }
}

#[test]
fn a_project_in_a_repository_has_one() {
    let temp = Temp::new("open");
    temp.commit("a.txt", "one\n", "first");

    let panel = Panel::open(&temp);
    assert!(panel.git.is_repository());
    assert_eq!(panel.git.head(), "main");
}

#[test]
fn a_project_that_is_not_in_one_says_so_rather_than_failing() {
    // Which is every project opened outside a repository, and the panel must simply not open.
    let directory = std::env::temp_dir().join(format!("zdt-gitui-none-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("a directory");

    let window = Window::open();
    let git = window
        .scope
        .with(|| GitUi::new(&directory, Rc::new(Nowhere)));
    // A temporary directory can itself be inside somebody's repository. This asserts only that
    // asking is safe.
    let _ = git.is_repository();
    git.refresh();

    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn what_has_changed_is_what_the_panel_lists() {
    let temp = Temp::new("status");
    temp.commit("a.txt", "one\n", "first");
    temp.write("a.txt", "one\ntwo\n");
    temp.write("new.txt", "fresh\n");

    let panel = Panel::open(&temp);
    let unstaged = panel.git.unstaged();
    let names: Vec<&str> = unstaged.iter().map(|one| one.path.as_str()).collect();

    assert!(names.contains(&"a.txt"), "the changed one: {names:?}");
    assert!(names.contains(&"new.txt"), "and the new one: {names:?}");
    assert!(panel.git.staged().is_empty(), "nothing is staged yet");
}

#[test]
fn staging_a_file_moves_it_to_the_other_list() {
    let temp = Temp::new("stage");
    temp.commit("a.txt", "one\n", "first");
    temp.write("a.txt", "one\ntwo\n");

    let panel = Panel::open(&temp);
    assert_eq!(panel.git.unstaged().len(), 1);

    panel.with(|git| {
        git.set_list(List::Unstaged);
        git.stage();
    });
    panel.settle();

    assert!(panel.git.unstaged().is_empty(), "it left the unstaged list");
    assert_eq!(panel.git.staged().len(), 1, "and joined the staged one");
    // And git itself agrees, which is the assertion that actually matters.
    assert!(
        temp.run(&["diff", "--cached", "--name-only"])
            .contains("a.txt")
    );
}

#[test]
fn unstaging_it_moves_it_back() {
    let temp = Temp::new("unstage");
    temp.commit("a.txt", "one\n", "first");
    temp.write("a.txt", "one\ntwo\n");
    temp.run(&["add", "a.txt"]);

    let panel = Panel::open(&temp);
    assert_eq!(panel.git.staged().len(), 1);

    panel.with(|git| {
        git.set_list(List::Staged);
        git.unstage();
    });
    panel.settle();

    assert!(panel.git.staged().is_empty());
    assert_eq!(panel.git.unstaged().len(), 1);
}

#[test]
fn the_history_is_what_was_committed() {
    let temp = Temp::new("history");
    temp.commit("a.txt", "one\n", "first");
    temp.commit("b.txt", "two\n", "second");
    temp.commit("c.txt", "three\n", "third");

    let panel = Panel::open(&temp);
    let summaries: Vec<String> = panel
        .git
        .commits()
        .into_iter()
        .map(|one| one.summary)
        .collect();
    assert_eq!(summaries, ["third", "second", "first"]);

    // And every commit has somewhere to be drawn.
    assert_eq!(panel.git.rows().len(), 3);
}

#[test]
fn the_caret_walks_the_list_it_is_in_and_stops_at_the_ends() {
    // Clamped, and never wrapped. Somebody reads these lists in order, and a `j` at the bottom
    // that jumps to the top loses their place.
    let temp = Temp::new("walk");
    temp.commit("a.txt", "one\n", "first");
    temp.commit("b.txt", "two\n", "second");

    let panel = Panel::open(&temp);
    panel.with(|git| {
        git.toggle_view();
        assert_eq!(git.view(), View::History);
        assert_eq!(git.at(List::History), 0);

        git.step(1);
        assert_eq!(git.at(List::History), 1);
        git.step(1);
        assert_eq!(git.at(List::History), 1, "it stops at the bottom");
        git.step(-5);
        assert_eq!(git.at(List::History), 0, "and at the top");
    });
}

#[test]
fn the_keys_move_between_the_lists() {
    let temp = Temp::new("focus");
    temp.commit("a.txt", "one\n", "first");

    let panel = Panel::open(&temp);
    panel.with(|git| {
        git.set_list(List::Unstaged);
        git.cycle_list(true);
        assert_eq!(git.list(), List::Staged);
        git.cycle_list(true);
        assert_eq!(git.list(), List::Diff);
        // And round again, so no list is unreachable.
        git.cycle_list(true);
        git.cycle_list(true);
        assert_eq!(git.list(), List::Unstaged);
    });
}

#[test]
fn what_is_selected_follows_the_list_the_keys_are_in() {
    let temp = Temp::new("selected");
    temp.commit("a.txt", "one\n", "first");
    temp.write("a.txt", "one\ntwo\n");

    let panel = Panel::open(&temp);
    panel.with(|git| {
        git.set_list(List::Unstaged);
        assert_eq!(
            git.selected(),
            Selected::File {
                path: "a.txt".to_owned(),
                staged: false,
            }
        );

        git.toggle_view();
        assert!(
            matches!(git.selected(), Selected::Commit(_)),
            "on the history side it is a commit"
        );
    });
}

#[test]
fn the_diff_is_of_whatever_is_selected() {
    let temp = Temp::new("diff");
    temp.commit("a.txt", "one\ntwo\n", "first");
    temp.write("a.txt", "one\nTWO\n");

    let panel = Panel::open(&temp);
    panel.with(|git| {
        git.set_list(List::Unstaged);
        git.load_diff();
    });
    panel.settle();

    let diff = panel.git.diff();
    assert_eq!(diff.len(), 1, "one file");
    assert_eq!(diff[0].path, "a.txt");
    assert_eq!(diff[0].counts(), (1, 1), "one line each way");
}

#[test]
fn committing_what_is_staged_makes_a_commit() {
    let temp = Temp::new("commit");
    temp.commit("a.txt", "one\n", "first");
    temp.write("a.txt", "one\ntwo\n");
    temp.run(&["add", "a.txt"]);

    let panel = Panel::open(&temp);
    panel.with(|git| git.commit("the second one"));
    panel.settle();

    assert_eq!(
        temp.run(&["log", "-1", "--format=%s"]).trim(),
        "the second one"
    );
    assert!(
        temp.run(&["status", "--porcelain"]).trim().is_empty(),
        "and nothing is left over"
    );
}

#[test]
fn the_branches_are_listed_with_the_current_one_marked() {
    let temp = Temp::new("branches");
    temp.commit("a.txt", "one\n", "first");
    temp.run(&["branch", "side"]);

    let panel = Panel::open(&temp);
    let branches = panel.git.branches();
    let names: Vec<&str> = branches.iter().map(|one| one.name.as_str()).collect();
    assert_eq!(names, ["main", "side"]);
    assert!(
        branches.iter().filter(|one| one.current).count() == 1,
        "exactly one is checked out"
    );
}

#[test]
fn a_repository_with_no_commits_opens_without_complaint() {
    // A project somebody has just run `git init` in, which is when a panel is least useful and
    // most likely to be opened by accident.
    let temp = Temp::new("empty");
    temp.write("a.txt", "one\n");

    let panel = Panel::open(&temp);
    assert!(panel.git.commits().is_empty());
    assert!(panel.git.branches().is_empty());
    assert_eq!(
        panel.git.unstaged().len(),
        1,
        "the untracked file is listed"
    );
}

// ---- The diff, flattened ------------------------------------------------------------------

#[test]
fn a_diff_flattens_to_one_row_per_line() {
    // Which is what lets it be drawn by a virtual list: a list has to know how many rows there are
    // without building any of them, and a structure of files holding hunks holding lines cannot
    // say.
    use zdt_gitui::{DiffRow, diff_rows};

    let diff = zdt_git::diff::between("a.txt", Some(b"one\ntwo\n"), Some(b"one\nTWO\n"));
    let rows = diff_rows(std::slice::from_ref(&diff));

    assert!(matches!(rows[0], DiffRow::File { .. }), "the heading first");
    assert!(
        matches!(rows[1], DiffRow::Hunk { .. }),
        "then the hunk's own line"
    );
    let lines = rows
        .iter()
        .filter(|row| matches!(row, DiffRow::Line { .. }))
        .count();
    assert_eq!(lines, diff.hunks[0].lines.len());
}

#[test]
fn every_row_of_a_hunk_says_which_hunk_it_is() {
    // Because `s` stages the hunk the caret's *row* is in, and a row that did not know would stage
    // the wrong one.
    use zdt_gitui::{DiffRow, diff_rows};

    let old: String = (0..40).map(|n| format!("line {n}\n")).collect();
    let mut changed: Vec<String> = (0..40).map(|n| format!("line {n}")).collect();
    changed[2] = "top".to_owned();
    changed[35] = "bottom".to_owned();
    let new: String = changed.iter().map(|line| format!("{line}\n")).collect();

    let diff = zdt_git::diff::between("a.txt", Some(old.as_bytes()), Some(new.as_bytes()));
    assert_eq!(diff.hunks.len(), 2);

    let rows = diff_rows(std::slice::from_ref(&diff));
    let hunks: Vec<usize> = rows.iter().filter_map(DiffRow::hunk).collect();
    assert!(
        hunks.contains(&0) && hunks.contains(&1),
        "both hunks are numbered"
    );
    assert!(
        rows.iter().any(|row| matches!(row, DiffRow::File { .. })),
        "and the file heading belongs to neither"
    );
}

#[test]
fn the_caret_walks_the_diff_a_line_at_a_time() {
    // Rows, and not hunks, so a long hunk can be read. What `s` stages is still a whole hunk.
    let temp = Temp::new("diff-walk");
    temp.commit("a.txt", "one\ntwo\nthree\n", "first");
    temp.write("a.txt", "one\nTWO\nthree\n");

    let panel = Panel::open(&temp);
    panel.with(|git| {
        git.set_list(List::Unstaged);
        git.load_diff();
    });
    panel.settle();

    panel.with(|git| {
        git.set_list(List::Diff);
        assert!(
            git.diff_rows().len() > 2,
            "a heading, a hunk line and its lines"
        );

        git.step(1);
        assert_eq!(git.at(List::Diff), 1);
        // And whatever row it lands on, the hunk it would stage is a real one.
        git.step(1);
        assert!(git.current_hunk().is_some());
    });
}
