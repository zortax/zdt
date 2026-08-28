//! Switching a split between source and rich form, and reading the rich form.

use crate::rich::{Previews, RichKind};
use crate::vim::Vim;
use crate::workspace::Workspace;

pub(super) fn run(workspace: &Workspace, vim: &Vim, leaf: &str) {
    match leaf {
        "toggle" => toggle(workspace, vim),
        _ => scroll(workspace, leaf),
    }
}

/// Flips the focused split between the source and the rich form of what it shows.
fn toggle(workspace: &Workspace, vim: &Vim) {
    let window = workspace.focused_untracked();
    let Some(buffer) = workspace.buffer_in_untracked(window) else {
        return;
    };
    let kind = workspace
        .buffer_untracked(buffer)
        .and_then(|entry| RichKind::of(&entry));
    let Some(kind) = kind else {
        workspace.say("this buffer has no rich view");
        return;
    };
    if !kind.has_source() {
        workspace.say("this buffer has only a rich view");
        return;
    }
    if !workspace.is_rich_untracked(window, buffer) {
        // The page has no caret, so whatever the engine was in the middle of ends here.
        vim.reset();
    }
    workspace.toggle_rich(window, buffer);
}

/// Reading the preview under the keyboard.
///
/// Only reachable while a rich view has the keyboard, through its region keymap. Every one of
/// these is a scroll, the same set the documentation panel answers.
fn scroll(workspace: &Workspace, leaf: &str) {
    let Some(previews) = zgui::reactive::use_local_context::<Previews>() else {
        return;
    };
    let Some(reading) = previews.current(workspace) else {
        return;
    };
    let page = reading.page();

    match leaf {
        "down" => reading.scroll_lines(1.0),
        "up" => reading.scroll_lines(-1.0),
        "half_down" => reading.scroll_by(page / 2.0),
        "half_up" => reading.scroll_by(-page / 2.0),
        "page_down" => reading.scroll_by(page),
        "page_up" => reading.scroll_by(-page),
        "top" => reading.to_top(),
        "bottom" => reading.to_bottom(),
        // Silently. The base map layers underneath the region, and an unbound key there falls
        // through to it.
        _ => {}
    }
}
