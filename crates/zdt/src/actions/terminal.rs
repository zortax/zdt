//! The terminals.

use crate::vim::Vim;
use crate::workspace::{Axis, Workspace};

/// The terminals.
///
/// One action, `terminal.toggle`, and the arguments say which one: a float by name, or a window
/// split with a shell in it. The names come from the keymap, so somebody who wants a `k9s` float
/// adds a row to a file.
pub(super) fn run(workspace: &Workspace, vim: &Vim, leaf: &str, args: &zdt_vim::Args) {
    use crate::terminals::{Program, Terminals};

    let Some(terminals) = zgui::reactive::use_local_context::<Terminals>() else {
        return;
    };

    match leaf {
        "toggle" => {
            let program = match args.str("command") {
                Some(line) => Program::command(line),
                None => Program::shell(),
            };
            match args.str("placement").unwrap_or("float") {
                "float" => {
                    // The name is what makes it the *same* float each time: without one, every
                    // press would start another lazygit.
                    let name = args.str("id").unwrap_or("default");
                    terminals.toggle_float(name, &program);
                }
                placement => {
                    // A split with a terminal in it, which is vim's `:sp | terminal`.
                    let axis = if placement == "vertical" {
                        Axis::Horizontal
                    } else {
                        Axis::Vertical
                    };
                    workspace.split(axis);
                    vim.reset();
                    if let Some(id) = terminals.open(&program) {
                        // The split was made for this terminal, so it goes when the terminal does.
                        terminals.owns_window(id, workspace.focused_untracked());
                        terminals.start_typing(id);
                    }
                }
            }
        }
        "open" => {
            let program = match args.str("command") {
                Some(line) => Program::command(line),
                None => Program::shell(),
            };
            if let Some(id) = terminals.open(&program) {
                terminals.start_typing(id);
            }
        }
        "normal" => terminals.stop_typing(),
        "hide" => terminals.hide_float(),
        "insert" => {
            if let Some(buffer) = workspace
                .current_buffer()
                .filter(|buffer| buffer.is_terminal())
            {
                terminals.start_typing(buffer.id);
            }
        }
        other => workspace.say(format!("terminal.{other} is not built yet")),
    }
}
