//! The window commands: splitting, closing, and moving between them.

use crate::explorer::Explorer;
use crate::focus::Focus;
use crate::workspace::{Axis, Direction, Workspace};

/// The window commands.
///
/// Nothing here puts the modal layer back to normal. Moving the keyboard is what does that, and
/// the focus is where that rule lives — see [`crate::focus::Focusing::enter_window`].
pub(super) fn run(workspace: &Workspace, leaf: &str, args: &zdt_vim::Args) {
    match leaf {
        "split" => {
            let axis = match args.str("axis") {
                Some("vertical") => Axis::Horizontal,
                _ => Axis::Vertical,
            };
            workspace.split(axis);
        }
        "close" => {
            if !workspace.close_window() {
                workspace.complain("the last window does not close");
            }
        }
        "cycle" => {
            workspace.cycle_window(true);
        }
        "focus" => {
            let Some(direction) = args.str("direction").and_then(Direction::named) else {
                // No direction named: `<C-w>w`, which walks the windows in order.
                workspace.cycle_window(true);
                return;
            };

            let explorer = zgui::reactive::use_local_context::<Explorer>();
            let focus = workspace.focus();

            let agent = zdt_agentui::try_use_agent();
            let agent_open = agent.as_ref().is_some_and(zdt_agentui::AgentUi::is_open);

            // A match and no ladder: a place that takes the keyboard added later fails to build
            // here, rather than being silently walked past.
            match focus.current_untracked() {
                // The tree is no window, so no amount of walking the layout finds it, and a person
                // in it pressing `<C-l>` means the panes.
                Focus::Tree => {
                    if direction == Direction::Left {
                        // Left of the tree sits the agent sidebar, when it is open.
                        if agent_open {
                            focus.enter_agent();
                        }
                    } else {
                        focus.enter_panes();
                    }
                }
                // The agent sidebar is the leftmost thing there is; the way out is to the right.
                Focus::Agent => {
                    if direction != Direction::Left {
                        if explorer.as_ref().is_some_and(Explorer::is_open) {
                            focus.enter_tree();
                        } else {
                            focus.enter_panes();
                        }
                    }
                }
                Focus::Window(_) => {
                    if !workspace.focus_direction(direction) && direction == Direction::Left {
                        // Nothing that way among the windows. To the left sit the tree and the
                        // agent sidebar, in that order.
                        if let Some(explorer) = explorer
                            && explorer.is_open()
                        {
                            focus.enter_tree();
                        } else if agent_open {
                            focus.enter_agent();
                        }
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
