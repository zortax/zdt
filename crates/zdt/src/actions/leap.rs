//! Leaping, and the editor's own keys.

use crate::vim::Vim;
use crate::workspace::Workspace;
use zgui_editor::EditorHandle;

/// Leap.
///
/// One action, and the argument says which way it looks. Everything after the key that started
/// it belongs to the leap layer.
pub(super) fn run(workspace: &Workspace, vim: &Vim, leaf: &str, handle: Option<&EditorHandle>) {
    use zdt_vim::leap::Direction;

    // A leap needs text to label and an editor to take its next two keys. Started from the tree or
    // a terminal there is neither, and what it would leave behind is a leap nothing can finish
    // that then swallows the first key typed back in the editor.
    if handle.is_none() {
        workspace.say("nothing to leap over here");
        return;
    }

    match leaf {
        "forward" => vim.start_leap(Direction::Forward),
        "backward" => vim.start_leap(Direction::Backward),
        // `gs` leaps into another window in leap.nvim. Here there is one window's worth of
        // labels, until the panes can say where each other's text is on screen. So it leaps both
        // ways in this one, which is the useful half of it, and it says so.
        "window" => vim.start_leap(Direction::Both),
        other => workspace.say(format!("leap.{other} is not built yet")),
    }
}

/// The few things that are the editor's own.
pub(super) fn editor(handle: Option<&EditorHandle>, leaf: &str) {
    if leaf == "focus"
        && let Some(handle) = handle
    {
        handle.focus();
    }
}
