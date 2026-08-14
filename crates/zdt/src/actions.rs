//! What every named action does.
//!
//! The engine knows `motion.word_forward` and `operator.delete`. Everything else — the pickers,
//! the buffers, the windows, the language servers — reaches here as a name and some arguments,
//! straight out of the keymap file. One `match` is the whole registry.
//!
//! An action nobody has written yet says so in the status line rather than doing nothing, which is
//! what makes a half-built editor say which half.

use zdt_vim::Action;
use zgui_editor::EditorHandle;

use crate::vim::Vim;
use crate::workspace::{Axis, Workspace};

/// Carries out `action`.
pub fn run(workspace: &Workspace, vim: &Vim, action: &Action, handle: &EditorHandle) {
    let leaf = action.leaf();
    let args = &action.args;

    match action.name.split('.').next().unwrap_or("") {
        "buffer" => buffer(workspace, leaf, args),
        "window" => window(workspace, vim, leaf, args),
        "app" => app(workspace, leaf),
        "editor" => editor(handle, leaf),
        // Everything else belongs to a part of the editor that is still being built. Saying so is
        // better than a key that quietly does nothing.
        _ => workspace.say(format!("{} is not built yet", action.name)),
    }
}

/// The buffer commands.
fn buffer(workspace: &Workspace, leaf: &str, args: &zdt_vim::Args) {
    match leaf {
        "save" => {
            if let Some(buffer) = workspace.current_buffer() {
                crate::files::save(workspace, buffer.id);
            }
        }
        "new" => {
            workspace.open_document(None, zgui_editor::Document::new(""));
        }
        "close" => {
            if let Some(buffer) = workspace.current_buffer() {
                if buffer.is_dirty() && !args.flag("force") {
                    workspace.complain("unsaved changes; <Leader>C closes anyway");
                } else {
                    workspace.close_buffer(buffer.id);
                }
            }
        }
        "next" => workspace.cycle_buffer(1),
        "previous" => workspace.cycle_buffer(-1),
        "alternate" => workspace.show_alternate(),
        "move" => workspace.move_buffer(args.number("offset").unwrap_or(1) as isize),
        "close_others" | "close_all" | "close_left" | "close_right" => {
            close_many(workspace, leaf);
        }
        "sort" => workspace.say(format!(
            "sorting by {} is not built yet",
            args.str("by").unwrap_or("")
        )),
        other => workspace.say(format!("buffer.{other} is not built yet")),
    }
}

/// The four ways of closing several buffers at once.
fn close_many(workspace: &Workspace, leaf: &str) {
    let order = workspace.order();
    let Some(current) = workspace.current_buffer().map(|buffer| buffer.id) else {
        return;
    };
    let Some(at) = order.iter().position(|held| *held == current) else {
        return;
    };

    let doomed: Vec<_> = match leaf {
        "close_others" => order
            .iter()
            .filter(|held| **held != current)
            .copied()
            .collect(),
        "close_all" => order.clone(),
        "close_left" => order[..at].to_vec(),
        "close_right" => order[at + 1..].to_vec(),
        _ => Vec::new(),
    };

    // Those with unsaved changes are kept, and said so: closing several at once must not be a way
    // to lose work by accident.
    let mut kept = 0;
    for id in doomed {
        let dirty = workspace
            .buffer_untracked(id)
            .is_some_and(|buffer| buffer.is_dirty());
        if dirty {
            kept += 1;
        } else {
            workspace.close_buffer(id);
        }
    }
    if kept > 0 {
        workspace.complain(format!("{kept} with unsaved changes were kept"));
    }
}

/// The window commands.
fn window(workspace: &Workspace, vim: &Vim, leaf: &str, args: &zdt_vim::Args) {
    match leaf {
        "split" => {
            let axis = match args.str("axis") {
                Some("vertical") => Axis::Horizontal,
                _ => Axis::Vertical,
            };
            workspace.split(axis);
            vim.reset();
        }
        "close" => {
            if !workspace.close_window() {
                workspace.complain("the last window does not close");
            } else {
                vim.reset();
            }
        }
        "cycle" => {
            workspace.cycle_window(true);
            vim.reset();
        }
        // Which window is left, below, above or right of this one needs the geometry of the frame
        // that was drawn, which the panes know and this does not yet.
        "focus" => {
            workspace.cycle_window(!matches!(args.str("direction"), Some("left" | "up")));
            vim.reset();
        }
        other => workspace.say(format!("window.{other} is not built yet")),
    }
}

/// The application itself.
fn app(workspace: &Workspace, leaf: &str) {
    match leaf {
        "quit" => {
            let unsaved = workspace
                .order()
                .into_iter()
                .filter(|id| {
                    workspace
                        .buffer_untracked(*id)
                        .is_some_and(|buffer| buffer.is_dirty())
                })
                .count();
            if unsaved > 0 {
                workspace.complain(format!("{unsaved} buffers have unsaved changes"));
            } else if let Some(windows) =
                zgui::reactive::use_local_context::<zgui::runtime::windows::Windows>()
            {
                windows.quit();
            }
        }
        other => workspace.say(format!("app.{other} is not built yet")),
    }
}

/// The few things that are the editor's own.
fn editor(handle: &EditorHandle, leaf: &str) {
    if leaf == "focus" {
        handle.focus();
    }
}
