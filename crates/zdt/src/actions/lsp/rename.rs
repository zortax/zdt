//! Renaming a symbol everywhere it appears.

use crate::actions::lsp::shared::*;
use crate::language::Language;
use crate::workspace::Workspace;
use zgui_editor::EditorHandle;

/// `<Leader>lr`.
///
/// Asks the server what exactly would be renamed, opens a box over it, and applies whatever comes
/// back. Asking first is what lets a key pressed on a keyword say "no" before somebody has typed a
/// new name for it.
pub(super) fn rename(workspace: &Workspace, language: &Language, handle: Option<&EditorHandle>) {
    let Some((handle, path)) = editing(workspace, handle) else {
        return;
    };
    let Some(mut client) = client(workspace, language, &path) else {
        return;
    };
    let position = caret_position(&handle, language, &path);
    let encoding = encoding_for(language, &path);

    // The word under the caret, as the fallback and as what the box opens holding. Read now,
    // because by the time the server answers the caret may have moved.
    let here = handle.query(|snapshot| {
        let caret = snapshot.selections().primary().head;
        snapshot.word_at(caret)
    });
    let box_of = zgui::reactive::use_local_context::<crate::rename::Rename>();

    let workspace = workspace.clone();
    zdt_view::detached(async move {
        let asked = {
            let path = path.clone();
            zgui::task::background(async move { client.prepare_rename(&path, position).await })
                .await
        };

        // What the server said would be renamed, or the word under the caret when it said
        // nothing. A server that refuses outright is saying this cannot be renamed, which is
        // worth hearing before a name has been typed.
        let (range, name) = match asked {
            Ok(Some(lsp_types::PrepareRenameResponse::Range(range))) => {
                let bytes = handle
                    .query(|snapshot| zdt_lsp::convert::range_of(snapshot.rope(), range, encoding));
                let text = handle.query(|snapshot| snapshot.text_in(bytes.clone()));
                (bytes, text)
            }
            Ok(Some(lsp_types::PrepareRenameResponse::RangeWithPlaceholder {
                range,
                placeholder,
            })) => {
                let bytes = handle
                    .query(|snapshot| zdt_lsp::convert::range_of(snapshot.rope(), range, encoding));
                (bytes, placeholder)
            }
            Ok(Some(lsp_types::PrepareRenameResponse::DefaultBehavior { .. })) | Ok(None) => {
                if here.is_empty() {
                    workspace.say("nothing to rename here");
                    return;
                }
                let text = handle.query(|snapshot| snapshot.text_in(here.clone()));
                (here, text)
            }
            Err(error) => {
                workspace.complain(error.to_string());
                return;
            }
        };

        let Some(panel) = box_of else {
            return;
        };
        let Some(rect) = handle.point_for_byte(range.start) else {
            workspace.say("the symbol is off screen");
            return;
        };
        panel.open(&name, range, rect);
    });
}

/// Carries out a rename that somebody has typed a name into.
///
/// Called by the rename box when it is accepted. The box is what knows the typed name.
pub fn rename_to(workspace: &Workspace, to: &str) {
    let Some(language) = zgui::reactive::use_local_context::<Language>() else {
        return;
    };
    let Some(handle) = workspace.current_handle() else {
        return;
    };
    let Some(path) = workspace.current_buffer().and_then(|buffer| buffer.path) else {
        return;
    };
    let Some(mut client) = client(workspace, &language, &path) else {
        return;
    };
    let position = caret_position(&handle, &language, &path);
    let encoding = encoding_for(&language, &path);
    let notify = crate::notify::use_notify();
    let to = to.to_owned();

    let workspace = workspace.clone();
    zdt_view::detached(async move {
        let found = {
            let path = path.clone();
            zgui::task::background(async move { client.rename(&path, position, &to).await }).await
        };
        match found {
            Ok(Some(edit)) => {
                crate::actions::edit::apply(&workspace, notify.as_ref(), edit, encoding);
            }
            Ok(None) => workspace.say("nothing was renamed"),
            Err(error) => match notify {
                Some(notify) => notify.fail("rename", Some(error.to_string())),
                None => workspace.complain(error.to_string()),
            },
        }
    });
}
