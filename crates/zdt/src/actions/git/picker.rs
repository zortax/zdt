//! The three git pickers, and checking a branch out.

use crate::git::Git;
use crate::workspace::Workspace;

/// The three git pickers: what has changed, what was committed, and what branches there are.
///
/// A picker, and not the panel. Somebody pressing these wants to *go* somewhere: to the file, to
/// the commit, onto the branch. Going places is what a picker is for. The panel is for looking at
/// a repository, and these are for leaving one.
pub fn picker(workspace: &Workspace, leaf: &str) {
    use crate::picker::{Deed, Picker, Row, Source, Target};

    let Some(picker) = zgui::reactive::use_local_context::<Picker>() else {
        return;
    };
    let root = workspace.project().root().to_path_buf();
    let which = leaf.to_owned();
    let workspace = workspace.clone();

    zdt_view::detached(async move {
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
                        let (mark, tint) = zdt_gitui::state_mark(state);
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
                                zdt_gitui::ago_short(commit.when)
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
pub(super) fn checkout(workspace: &Workspace, root: &std::path::Path, name: &str) {
    let (root, name, workspace) = (root.to_path_buf(), name.to_owned(), workspace.clone());
    let notify = crate::notify::use_notify();
    let signs = zgui::reactive::use_local_context::<Git>();
    zdt_view::detached(async move {
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
