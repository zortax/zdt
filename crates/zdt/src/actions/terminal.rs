//! The terminals.

use crate::vim::Vim;
use crate::workspace::{Axis, BufferId, Workspace};

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
                    if let Some(id) = terminals.open(&program) {
                        // The split was made for this terminal, so it goes when the terminal does.
                        terminals.owns_window(id, workspace.focused_untracked());
                    }
                }
            }
        }
        "open" => {
            let program = match args.str("command") {
                Some(line) => Program::command(line),
                None => Program::shell(),
            };
            terminals.open(&program);
        }
        // Whichever terminal is being looked at: the float when one is up, and the current buffer
        // otherwise. Which mode a terminal is in is a fact about that terminal, so leaving one
        // names one.
        "normal" => {
            if let Some(buffer) = looked_at(workspace, &terminals) {
                terminals.enter_normal_mode(vim, buffer);
            }
        }
        "insert" => {
            if let Some(buffer) = looked_at(workspace, &terminals) {
                terminals.enter_terminal_mode(vim, buffer);
            }
        }
        "hide" => terminals.hide_float(),
        other => workspace.say(format!("terminal.{other} is not built yet")),
    }
}

/// Which terminal a key about "the terminal" is about.
///
/// The float when one is showing, because it is over everything and has the keys. The current
/// buffer otherwise, when that buffer is a terminal.
fn looked_at(workspace: &Workspace, terminals: &crate::terminals::Terminals) -> Option<BufferId> {
    terminals.showing().or_else(|| {
        workspace
            .current_buffer()
            .filter(crate::workspace::Buffer::is_terminal)
            .map(|buffer| buffer.id)
    })
}
