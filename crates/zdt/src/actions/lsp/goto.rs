//! Going to what a symbol names, and finding what names it.

use crate::actions::lsp::shared::*;
use crate::actions::lsp::symbols::show_locations;
use crate::language::Language;
use crate::workspace::Workspace;
use zgui_editor::EditorHandle;

/// `gd`, `gD`, `gy` and `gI`.
///
/// One answer opens it. Several open a picker, because choosing between them is exactly what a
/// picker is for. The action's leaf says which question is asked. The four are different questions
/// in every language that has all of them, and a `gy` that answered `gd` was a placeholder.
pub(super) fn go_to(
    workspace: &Workspace,
    language: &Language,
    handle: Option<&EditorHandle>,
    which: &str,
) {
    let Some((handle, path)) = editing(workspace, handle) else {
        return;
    };
    let Some(mut client) = client(workspace, language, &path) else {
        return;
    };
    let position = caret_position(&handle, language, &path);
    let (title, nothing) = match which {
        "declaration" => ("Declarations", "no declaration"),
        "type_definition" => ("Type definitions", "no type definition"),
        "implementation" => ("Implementations", "no implementation"),
        _ => ("Definitions", "no definition"),
    };
    let which = which.to_owned();
    let picker = zgui::reactive::use_local_context::<crate::picker::Picker>();

    let workspace = workspace.clone();
    zdt_view::detached(async move {
        let found = {
            let path = path.clone();
            zgui::task::background(async move {
                match which.as_str() {
                    "declaration" => client.declaration(&path, position).await,
                    "type_definition" => client.type_definition(&path, position).await,
                    "implementation" => client.implementation(&path, position).await,
                    _ => client.definition(&path, position).await,
                }
            })
            .await
        };
        match found {
            Ok(locations) if locations.is_empty() => workspace.say(nothing),
            Ok(locations) if locations.len() == 1 => open_location(&workspace, &locations[0]),
            Ok(locations) => show_locations(&workspace, picker.clone(), title, &locations),
            Err(error) => workspace.complain(error.to_string()),
        }
    });
}

/// `gr`.
pub(super) fn references(
    workspace: &Workspace,
    language: &Language,
    handle: Option<&EditorHandle>,
) {
    let Some((handle, path)) = editing(workspace, handle) else {
        return;
    };
    let Some(mut client) = client(workspace, language, &path) else {
        return;
    };
    let position = caret_position(&handle, language, &path);
    let picker = zgui::reactive::use_local_context::<crate::picker::Picker>();

    let workspace = workspace.clone();
    zdt_view::detached(async move {
        let found = {
            let path = path.clone();
            zgui::task::background(async move { client.references(&path, position).await }).await
        };
        match found {
            Ok(locations) if locations.is_empty() => workspace.say("no references"),
            // One reference is the thing itself; going there beats a picker with one row.
            Ok(locations) if locations.len() == 1 => open_location(&workspace, &locations[0]),
            Ok(locations) => show_locations(&workspace, picker.clone(), "References", &locations),
            Err(error) => workspace.complain(error.to_string()),
        }
    });
}
