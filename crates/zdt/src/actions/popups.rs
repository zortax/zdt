//! Scrolling the popups: the documentation panel and the suggestions.

use crate::workspace::Workspace;
use zgui_editor::EditorHandle;

/// Reading the documentation panel.
///
/// Only reachable while the panel has the keyboard, which a second `K` gives it. See
/// [`crate::hover`]. Every one of these is a scroll, because reading is the only thing there is to
/// do with a panel of documentation.
pub(super) fn hover(leaf: &str) {
    let Some(panel) = zgui::reactive::use_local_context::<crate::hover::Hover>() else {
        return;
    };
    let page = panel.page();

    match leaf {
        "down" => panel.scroll_lines(1.0),
        "up" => panel.scroll_lines(-1.0),
        "half_down" => panel.scroll_by(page / 2.0),
        "half_up" => panel.scroll_by(-page / 2.0),
        "page_down" => panel.scroll_by(page),
        "page_up" => panel.scroll_by(-page),
        "top" => panel.to_top(),
        "bottom" => panel.to_bottom(),
        "close" => panel.hide(),
        // Silently. The overlay is only in front while the panel is up, and an unbound key there
        // falls through to the editor.
        _ => {}
    }
}

/// The suggestion popup.
pub(super) fn completion(workspace: &Workspace, leaf: &str, handle: Option<&EditorHandle>) {
    let Some(completion) = zgui::reactive::use_local_context::<crate::completion::Completion>()
    else {
        return;
    };

    match leaf {
        "next" => completion.step(1),
        "previous" => completion.step(-1),
        "accept" => completion.accept(handle),
        "cancel" => completion.close(),
        "docs_down" => completion.scroll_docs(1.0),
        "docs_up" => completion.scroll_docs(-1.0),
        "open" => completion.ask(workspace, handle),
        _ => {}
    }
}
