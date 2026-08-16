//! What the git keys do.
//!
//! Two kinds. The ones in this module act on the file being edited — walk to the next change,
//! stage the hunk under the caret, ask who last touched a line — and want an editor and a caret.
//! The ones that open the panel do not, and are answered first.
//!
//! What is still *not* here is anything that rewrites history: a rebase is a thing to be doing on
//! purpose with the whole of your attention, and a key in a text editor is not the way to start
//! one.

use zgui_editor::EditorHandle;

use crate::git::Git;
use crate::workspace::Workspace;

/// Carries out one `git.*` action.
pub fn run(workspace: &Workspace, leaf: &str, handle: Option<&EditorHandle>) {
    // The panel first, because none of these needs a file, a caret or a diff — and asking for one
    // would mean `<Leader>gg` did nothing in a window showing a terminal.
    if let Some(panel) = zgui::reactive::use_local_context::<crate::gitui::GitUi>() {
        match leaf {
            "open" | "status" => {
                panel.open();
                return;
            }
            "open_tab" => {
                panel.open_tab();
                return;
            }
            "close" => {
                panel.close();
                return;
            }
            _ => {}
        }
    }

    let Some(git) = zgui::reactive::use_local_context::<Git>() else {
        return;
    };
    let Some(handle) = handle else {
        return;
    };
    let Some(path) = workspace.current_buffer().and_then(|buffer| buffer.path) else {
        workspace.say("this buffer is not a file");
        return;
    };

    let line = handle.query(|snapshot| {
        let caret = snapshot.selections().primary().head;
        snapshot.rope().byte_to_line(caret)
    });
    let hunks = git.hunks(&path);

    match leaf {
        "next_hunk" | "previous_hunk" => {
            let found = if leaf == "next_hunk" {
                zdt_git::after(&hunks, line)
            } else {
                zdt_git::before(&hunks, line)
            };
            let Some(found) = found else {
                workspace.say("no changes");
                return;
            };
            let at = handle.query(|snapshot| {
                let rope = snapshot.rope();
                let line = found.line.min(rope.len_lines().saturating_sub(1));
                rope.char_to_byte(rope.line_to_char(line))
            });
            handle.command(zgui_editor::Command::SetSelections {
                selections: vec![zgui_editor::Selection::caret(at)],
                primary: 0,
            });
            handle.command(zgui_editor::Command::Scroll(
                zgui_editor::ScrollCmd::CursorCenter,
            ));
        }
        "preview_hunk" => match hunks.iter().find(|hunk| hunk.covers(line)) {
            Some(hunk) => workspace.say(format!(
                "{:?} at line {}, {} line{}",
                hunk.change,
                hunk.line + 1,
                hunk.count.max(1),
                if hunk.count == 1 { "" } else { "s" }
            )),
            None => workspace.say("nothing changed here"),
        },
        // Staging the hunk under the caret, without opening anything. The hunks the gutter holds
        // are the cheap `git diff` kind and say only where a change is, so the one that is wanted
        // is found again properly — which is the same work the panel does, from the same crate.
        "stage_hunk" | "reset_hunk" => {
            let Some(hunk) = hunks.iter().find(|hunk| hunk.covers(line)) else {
                workspace.say("nothing changed here");
                return;
            };
            let at = hunk.line;
            let staging = leaf == "stage_hunk";
            let root = workspace.project().root().to_path_buf();
            let path = path.clone();
            let workspace = workspace.clone();
            // Taken now: neither a context nor a notify is reachable after the await below.
            let notify = crate::notify::use_notify();
            let signs = zgui::reactive::use_local_context::<Git>();

            crate::task::detached(async move {
                let done = zgui::task::blocking(move || {
                    let repo = zdt_git::Repo::open(&root)?;
                    let named = repo
                        .relative(&path)
                        .ok_or_else(|| zdt_git::Error::NotARepository(path.clone()))?;
                    let found = if staging {
                        zdt_git::diff::worktree(&repo, &named)?
                    } else {
                        zdt_git::diff::staged(&repo, &named)?
                    };
                    // The one covering the line the caret is on, matched by where it starts in
                    // whichever file it is a diff of.
                    let wanted = found.hunks.into_iter().find(|hunk| {
                        let start = if staging {
                            hunk.new_start
                        } else {
                            hunk.old_start
                        };
                        let count = if staging {
                            hunk.new_count
                        } else {
                            hunk.old_count
                        };
                        let from = start.saturating_sub(1) as usize;
                        (from..from + count.max(1) as usize).contains(&at)
                    });
                    let Some(wanted) = wanted else {
                        return Ok(false);
                    };
                    if staging {
                        zdt_git::stage::stage_hunks(&repo, &named, &[wanted])?;
                    } else {
                        zdt_git::stage::unstage_hunks(&repo, &named, &[wanted])?;
                    }
                    Ok::<bool, zdt_git::Error>(true)
                })
                .await;

                match done {
                    Ok(true) => {
                        let said = if staging {
                            "hunk staged"
                        } else {
                            "hunk unstaged"
                        };
                        match notify.as_ref() {
                            Some(notify) => notify.say(said),
                            None => workspace.say(said),
                        }
                        // The gutter is worked out from `git diff`, which has just changed.
                        if let Some(signs) = signs {
                            signs.refresh_path(
                                &workspace
                                    .current_buffer()
                                    .and_then(|buffer| buffer.path)
                                    .unwrap_or_default(),
                            );
                        }
                    }
                    Ok(false) => workspace.say("nothing changed here"),
                    Err(error) => match notify.as_ref() {
                        Some(notify) => notify.fail("git", Some(error.to_string())),
                        None => workspace.complain(error.to_string()),
                    },
                }
            });
        }
        "blame_line" => blame(workspace, &path, line),
        other => workspace.say(format!("git.{other} is not built yet")),
    }
}

/// Who last touched a line, in the status line.
fn blame(workspace: &Workspace, path: &std::path::Path, line: usize) {
    let (path, workspace) = (path.to_path_buf(), workspace.clone());
    crate::task::detached(async move {
        let said = {
            let path = path.clone();
            zgui::task::blocking(move || blame_line(&path, line)).await
        };
        match said {
            Some(said) => workspace.say(said),
            None => workspace.say("no blame for this line"),
        }
    });
}

/// What `git blame` says about one line, as one line.
///
/// Blocking. Nothing when the file is not tracked or git is not installed, which are both "there
/// is nothing to say" rather than errors.
fn blame_line(path: &std::path::Path, line: usize) -> Option<String> {
    let directory = path.parent()?;
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(["blame", "--porcelain", "-L"])
        .arg(format!("{},{}", line + 1, line + 1))
        .arg("--")
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut author = None;
    let mut when = None;
    let mut summary = None;
    for said in text.lines() {
        if let Some(rest) = said.strip_prefix("author ") {
            author = Some(rest.to_owned());
        } else if let Some(rest) = said.strip_prefix("author-time ") {
            when = rest.parse::<i64>().ok();
        } else if let Some(rest) = said.strip_prefix("summary ") {
            summary = Some(rest.to_owned());
        }
    }

    let author = author?;
    let summary = summary.unwrap_or_default();
    match when {
        Some(when) => Some(format!("{author}, {} — {summary}", ago(when))),
        None => Some(format!("{author} — {summary}")),
    }
}

/// How long ago a unix timestamp was, roughly.
///
/// Roughly on purpose: "3 months ago" is what anybody reads off a blame line, and the exact day is
/// what `git log` is for.
fn ago(when: i64) -> String {
    let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) else {
        return "some time ago".to_owned();
    };
    let seconds = (now.as_secs() as i64 - when).max(0);

    let (count, unit) = match seconds {
        ..60 => return "just now".to_owned(),
        60..3_600 => (seconds / 60, "minute"),
        3_600..86_400 => (seconds / 3_600, "hour"),
        86_400..2_592_000 => (seconds / 86_400, "day"),
        2_592_000..31_536_000 => (seconds / 2_592_000, "month"),
        _ => (seconds / 31_536_000, "year"),
    };
    format!("{count} {unit}{} ago", if count == 1 { "" } else { "s" })
}

/// The three git pickers: what has changed, what was committed, and what branches there are.
///
/// A picker rather than the panel, because what somebody pressing these wants is to *go*
/// somewhere — to the file, to the commit, onto the branch — and going places is what a picker is
/// for. The panel is for looking at a repository; these are for leaving one.
pub fn picker(workspace: &Workspace, leaf: &str) {
    use crate::picker::{Deed, Picker, Row, Source, Target};

    let Some(picker) = zgui::reactive::use_local_context::<Picker>() else {
        return;
    };
    let root = workspace.project().root().to_path_buf();
    let which = leaf.to_owned();
    let workspace = workspace.clone();

    crate::task::detached(async move {
        let read = {
            let root = root.clone();
            let which = which.clone();
            zgui::task::blocking(move || {
                let repo = zdt_git::Repo::open(&root).ok()?;
                Some(match which.as_str() {
                    "git_status" => Answer::Status(zdt_git::status::status(&repo).ok()?),
                    "git_commits" => Answer::Commits(zdt_git::log::log(&repo, None, 500).ok()?),
                    _ => Answer::Branches(zdt_git::branches(&repo).ok()?),
                })
            })
            .await
        };

        let Some(read) = read else {
            workspace.say("this project is not in a git repository");
            return;
        };

        let (title, rows): (&'static str, Vec<Row>) = match read {
            Answer::Status(entries) => (
                "Git status",
                entries
                    .into_iter()
                    .map(|entry| {
                        // The worktree mark where there is one, and the index's otherwise: a file
                        // that is staged and unchanged since still wants a letter.
                        let state = if entry.worktree.is_change() {
                            entry.worktree
                        } else {
                            entry.index
                        };
                        let (mark, tint) = crate::gitui::state_mark(state);
                        Row::plain(
                            entry.path.clone(),
                            Target::File {
                                path: entry.full,
                                line: None,
                                matched: None,
                            },
                        )
                        .with_detail(mark)
                        .with_glyph(mark_glyph(mark), tint)
                    })
                    .collect(),
            ),
            Answer::Commits(commits) => (
                "Git commits",
                commits
                    .into_iter()
                    .map(|commit| {
                        Row::plain(commit.summary.clone(), Target::Nothing)
                            .with_detail(format!(
                                "{}  {}  {}",
                                commit.short,
                                commit.author,
                                crate::gitui::ago_short(commit.when)
                            ))
                            .with_glyph("\u{f0718}", "zdt-git-changed")
                    })
                    .collect(),
            ),
            Answer::Branches(branches) => (
                "Git branches",
                branches
                    .into_iter()
                    .map(|branch| {
                        let name = branch.name.clone();
                        let root = root.clone();
                        let workspace = workspace.clone();
                        Row::plain(
                            branch.name.clone(),
                            // Through git itself: a checkout writes the working tree, updates the
                            // index and moves `HEAD`, and doing two of those three correctly is
                            // worse than doing none of them.
                            Target::Run(Deed::new(move || {
                                checkout(&workspace, &root, &name);
                            })),
                        )
                        .with_detail(branch.upstream.unwrap_or_default())
                        .with_glyph(
                            if branch.current {
                                "\u{25cf}"
                            } else {
                                "\u{f062c}"
                            },
                            if branch.current {
                                "zdt-git-added"
                            } else {
                                "zui-color-muted-foreground"
                            },
                        )
                    })
                    .collect(),
            ),
        };

        if rows.is_empty() {
            workspace.say("nothing to show");
            return;
        }
        picker.open(Source::Given { title, rows });
    });
}

/// What one of the three questions came back with.
enum Answer {
    Status(Vec<zdt_git::Entry>),
    Commits(Vec<zdt_git::Commit>),
    Branches(Vec<zdt_git::Branch>),
}

/// Which glyph a status letter gets in the picker.
///
/// The letter itself is in the dim text after the name; the glyph is what carries the colour, and
/// one shape for every state is what keeps the column from looking like a ransom note.
const fn mark_glyph(mark: &str) -> &'static str {
    match mark.as_bytes() {
        b"?" => "\u{f0453}",
        b"A" => "\u{f0704}",
        b"D" => "\u{f0708}",
        b"R" => "\u{f070c}",
        b"U" => "\u{f071b}",
        _ => "\u{f0704}",
    }
}

/// Checks a branch out, and says what happened.
fn checkout(workspace: &Workspace, root: &std::path::Path, name: &str) {
    let (root, name, workspace) = (root.to_path_buf(), name.to_owned(), workspace.clone());
    let notify = crate::notify::use_notify();
    let signs = zgui::reactive::use_local_context::<Git>();
    crate::task::detached(async move {
        let done = {
            let name = name.clone();
            zgui::task::blocking(move || {
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(&root)
                    .args(["checkout", &name])
                    .output()
            })
            .await
        };

        match done {
            Ok(output) if output.status.success() => {
                match notify.as_ref() {
                    Some(notify) => notify.ok(format!("on {name}")),
                    None => workspace.say(format!("on {name}")),
                }
                // Every open file may have changed underneath, so the signs are worked out again.
                if let Some(signs) = signs {
                    for buffer in workspace.order() {
                        signs.refresh(buffer);
                    }
                }
            }
            Ok(output) => {
                let said = String::from_utf8_lossy(&output.stderr);
                let first = said
                    .lines()
                    .map(str::trim)
                    .find(|line| !line.is_empty())
                    .unwrap_or("git refused")
                    .to_owned();
                match notify.as_ref() {
                    Some(notify) => notify.fail("checkout", Some(first)),
                    None => workspace.complain(first),
                }
            }
            Err(error) => match notify.as_ref() {
                Some(notify) => notify.fail("checkout", Some(error.to_string())),
                None => workspace.complain(error.to_string()),
            },
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn how_long_ago_reads_in_the_largest_unit_that_fits() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is after 1970")
            .as_secs() as i64;

        assert_eq!(ago(now), "just now");
        assert_eq!(ago(now - 120), "2 minutes ago");
        assert_eq!(ago(now - 3_600), "1 hour ago");
        assert_eq!(ago(now - 86_400 * 3), "3 days ago");
        assert_eq!(ago(now - 2_592_000 * 4), "4 months ago");
        assert_eq!(ago(now - 31_536_000 * 2), "2 years ago");
    }

    #[test]
    fn a_timestamp_in_the_future_is_not_a_negative_age() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is after 1970")
            .as_secs() as i64;
        assert_eq!(ago(now + 10_000), "just now");
    }
}
