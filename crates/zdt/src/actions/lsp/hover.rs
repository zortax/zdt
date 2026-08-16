//! What the server says about the thing under the caret.

use crate::actions::lsp::shared::*;
use crate::language::Language;
use crate::workspace::Workspace;
use zgui_editor::EditorHandle;

/// `K`.
///
/// A panel anchored to the caret, holding the answer drawn as the markdown it is.
///
/// Pressed a second time while the panel is up it gives the panel the keyboard instead of asking
/// again: the second press means "I want to read this", and what somebody reading a panel needs is
/// to be able to scroll it. The request is not repeated, because the answer has not changed.
pub(super) fn hover(workspace: &Workspace, language: &Language, handle: Option<&EditorHandle>) {
    if let Some(panel) = zgui::reactive::use_local_context::<crate::hover::Hover>()
        && panel.is_showing()
        && panel.focus()
    {
        return;
    }

    let Some((handle, path)) = editing(workspace, handle) else {
        return;
    };
    let Some(mut client) = client(workspace, language, &path) else {
        return;
    };
    let position = caret_position(&handle, language, &path);
    // Taken *now*, while there is a scope to take it from. A context looked up after an await is
    // gone, and the panel would silently never open. See `tests/context.rs`.
    let panel = zgui::reactive::use_local_context::<crate::hover::Hover>();

    let workspace = workspace.clone();
    zdt_view::detached(async move {
        let found = {
            let path = path.clone();
            zgui::task::background(async move { client.hover(&path, position).await }).await
        };
        match found {
            Ok(Some(found)) => {
                // Where the caret is *now*: the answer took a round trip, and anchoring it to
                // where the caret was would put the panel somewhere nothing is.
                let at = handle.query(|snapshot| snapshot.selections().primary().head);
                match (panel, handle.point_for_byte(at)) {
                    (Some(panel), Some(rect)) => panel.show(&found.contents, rect),
                    // Off screen, or no panel: the first line in the status bar still says
                    // something, which beats a key that appears to do nothing.
                    _ => workspace.say(crate::hover::one_line(&crate::hover::markdown_of(
                        &found.contents,
                    ))),
                }
            }
            Ok(None) => workspace.say("nothing here"),
            Err(error) => workspace.complain(error.to_string()),
        }
    });
}
