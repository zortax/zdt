//! The buffer commands, and closing several at once.

use crate::workspace::Workspace;

/// The buffer commands.
pub(super) fn run(workspace: &Workspace, leaf: &str, args: &zdt_vim::Args) {
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
                } else if buffer.is_terminal() {
                    // A terminal's program has to be shut down as well, and the split it was
                    // opened in goes with it.
                    match zgui::reactive::use_local_context::<crate::terminals::Terminals>() {
                        Some(terminals) => terminals.end(workspace, buffer.id),
                        None => {
                            workspace.close_buffer(buffer.id);
                        }
                    }
                } else {
                    workspace.close_buffer(buffer.id);
                }
            }
        }
        // The tabs are already on screen with their names on them. A modal that covers them to
        // list them again is a worse way to do the same thing.
        "pick" | "pick_close" => {
            if let Some(tabs) = zgui::reactive::use_local_context::<crate::tabpick::TabPick>() {
                tabs.start(if leaf == "pick_close" {
                    crate::tabpick::Then::Close
                } else {
                    crate::tabpick::Then::Show
                });
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
pub(super) fn close_many(workspace: &Workspace, leaf: &str) {
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
