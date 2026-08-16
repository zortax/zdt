//! The file tree.

use crate::actions::files::{create, delete, paste, rename, with_settings};
use crate::explorer::Explorer;
use crate::workspace::Workspace;

/// The file tree.
///
/// Everything that touches the filesystem goes through a worker and reports back, because a
/// directory copy on the interface thread is a frozen window.
pub(super) fn run(workspace: &Workspace, leaf: &str, args: &zdt_vim::Args) {
    let Some(explorer) = zgui::reactive::use_local_context::<Explorer>() else {
        return;
    };

    match leaf {
        "toggle" => explorer.toggle(),
        "focus" => explorer.focus(),
        "close" => {
            explorer.close();
            workspace.focus_editor();
        }
        "leave" => {
            explorer.unfocus();
            workspace.focus_editor();
        }
        "down" => explorer.move_by(1),
        "up" => explorer.move_by(-1),
        "first" => explorer.go_to(0),
        "last" => explorer.go_to(usize::MAX),
        "parent_or_close" => explorer.parent_or_close(),
        // `<CR>` and a click work both ways. `l` steps into what is already open. The two differ
        // on purpose, as they do in neo-tree.
        //
        // This is `tree.open`, and never `tree.toggle`. That one is the panel itself, and a row
        // and a panel are two different things to toggle.
        "activate" => {
            if let Some(path) = explorer.toggle_selected() {
                crate::files::open(workspace, path);
                explorer.unfocus();
                workspace.focus_editor();
            }
        }
        "child_or_open" => {
            if let Some(path) = explorer.open_selected() {
                crate::files::open(workspace, path);
                // Opening a file gives the keyboard back, the way `<CR>` in neo-tree does.
                explorer.unfocus();
                workspace.focus_editor();
            }
        }
        "refresh" => explorer.refresh(),
        "reveal" => {
            if let Some(path) = workspace.current_buffer().and_then(|buffer| buffer.path) {
                explorer.focus();
                explorer.reveal(&path);
            }
        }
        // Both go through the settings, because the tree follows the settings. A write straight
        // to the tree would be undone the next time anything else changed.
        "toggle_hidden" => {
            let now = with_settings(|config| {
                config.tree.hidden = !config.tree.hidden;
                config.tree.hidden
            });
            if let Some(now) = now {
                workspace.say(if now {
                    "hidden files on"
                } else {
                    "hidden files off"
                });
            }
        }
        "toggle_ignored" => {
            let now = with_settings(|config| {
                config.tree.ignored = !config.tree.ignored;
                config.tree.ignored
            });
            if let Some(now) = now {
                workspace.say(if now {
                    "ignored files on"
                } else {
                    "ignored files off"
                });
            }
        }
        "copy_path" => {
            if let Some(row) = explorer.selected() {
                let path = row.entry.path.display().to_string();
                zgui::runtime::clipboard::use_clipboard()
                    .set_text(zgui::platform::ClipboardKind::Standard, path.clone());
                workspace.say(path);
            }
        }
        "copy" | "cut" => {
            let cut = leaf == "cut";
            if let Some(path) = explorer.hold(cut) {
                workspace.say(format!(
                    "{} {}",
                    if cut { "cut" } else { "copied" },
                    path.file_name().unwrap_or_default().to_string_lossy()
                ));
            }
        }
        "paste" => paste(workspace, &explorer),
        "create" => create(workspace, &explorer, args.flag("directory")),
        "rename" => rename(workspace, &explorer),
        "delete" => delete(workspace, &explorer),
        "system_open" => {
            if let Some(row) = explorer.selected() {
                let path = row.entry.path.clone();
                zdt_view::detached(async move {
                    zgui::task::blocking(move || {
                        let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
                    })
                    .await;
                });
            }
        }
        other => workspace.say(format!("tree.{other} is not built yet")),
    }
}
