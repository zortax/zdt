//! The window commands: splitting, closing, and moving between them.

use crate::explorer::Explorer;
use crate::focus::Focus;
use crate::vim::Vim;
use crate::workspace::{Axis, Direction, Workspace};

/// The window commands.
pub(super) fn run(workspace: &Workspace, vim: &Vim, leaf: &str, args: &zdt_vim::Args) {
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
        "focus" => {
            let Some(direction) = args.str("direction").and_then(Direction::named) else {
                // No direction named: `<C-w>w`, which walks the windows in order.
                workspace.cycle_window(true);
                vim.reset();
                return;
            };

            let explorer = zgui::reactive::use_local_context::<Explorer>();
            let focus = workspace.focus();

            // A match and no ladder: a place that takes the keyboard added later fails to build
            // here, rather than being silently walked past.
            match focus.current_untracked() {
                // The tree is no window, so no amount of walking the layout finds it, and a person
                // in it pressing `<C-l>` means the panes.
                Focus::Tree => {
                    if direction != Direction::Left {
                        focus.enter_panes();
                        vim.reset();
                    }
                }
                Focus::Window(_) => {
                    if workspace.focus_direction(direction) {
                        vim.reset();
                    } else if direction == Direction::Left
                        && let Some(explorer) = explorer
                        && explorer.is_open()
                    {
                        // Nothing that way among the windows. To the left that is the tree, the one
                        // thing beside them that takes the keyboard.
                        focus.enter_tree();
                        vim.reset();
                    }
                }
                // Something over the panes has the keys, so a window command never runs from here.
                Focus::Overlay(_) => {}
            }
        }
        "zoom" => {
            let step = args.number("step").unwrap_or(0) as i32;
            workspace.zoom(workspace.focused_untracked(), step);
        }
        other => workspace.say(format!("window.{other} is not built yet")),
    }
}
