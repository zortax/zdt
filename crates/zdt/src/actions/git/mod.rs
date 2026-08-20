//! What the git keys do.
//!
//! Two kinds. The ones in this module act on the file being edited, and want an editor and a
//! caret: walk to the next change, stage the hunk under the caret, ask who last touched a line.
//! The ones that open the panel need neither, and are answered first.
//!
//! Nothing here rewrites history. A rebase is a thing to do on purpose with the whole of your
//! attention, and a key in a text editor is no way to start one.

mod blame;
mod picker;

pub use crate::actions::git::picker::picker;

use zgui_editor::EditorHandle;

use crate::git::Git;
use crate::workspace::Workspace;

/// Carries out one `git.*` action.
pub fn run(workspace: &Workspace, leaf: &str, handle: Option<&EditorHandle>) {
    // The panel first, because none of these needs a file, a caret or a diff. Asking for one
    // would leave `<Leader>gg` doing nothing in a window showing a terminal.
    if let Some(panel) = zgui::reactive::use_local_context::<zdt_gitui::GitUi>() {
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
        // Staging the hunk under the caret, with nothing opened. The hunks the gutter holds are
        // the cheap `git diff` kind and say only where a change is. So the wanted hunk is read
        // again in full, which is the same work the panel does, from the same crate.
        "stage_hunk" | "reset_hunk" => {
            let Some(hunk) = hunks.iter().find(|hunk| hunk.covers(line)) else {
                workspace.say("nothing changed here");
                return;
            };
            let at = hunk.line;
            let staging = leaf == "stage_hunk";
            let root = workspace.project().tooling_root().to_path_buf();
            let path = path.clone();
            let workspace = workspace.clone();
            // Taken now: neither a context nor a notify is reachable after the await below.
            let notify = crate::notify::use_notify();
            let signs = zgui::reactive::use_local_context::<Git>();

            zdt_view::detached(async move {
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
        "blame_line" => self::blame::blame(workspace, &path, line),
        other => workspace.say(format!("git.{other} is not built yet")),
    }
}
