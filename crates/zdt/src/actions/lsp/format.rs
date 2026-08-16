//! Formatting a file, or the selection in one.

use crate::actions::lsp::shared::*;
use crate::language::Language;
use crate::workspace::Workspace;
use zgui_editor::EditorHandle;

/// `<Leader>lf`.
pub(super) fn format(workspace: &Workspace, language: &Language, handle: Option<&EditorHandle>) {
    let Some((handle, path)) = editing(workspace, handle) else {
        return;
    };
    let Some(mut client) = client(workspace, language, &path) else {
        return;
    };
    let (tab_size, spaces) = zgui::reactive::use_local_context::<crate::settings::Settings>()
        .map_or((4, true), |settings| {
            settings.with_untracked(|config| (config.editor.tab_size, config.editor.expand_tab))
        });
    let encoding = encoding_for(language, &path);

    let workspace = workspace.clone();
    zdt_view::detached(async move {
        let edits = {
            let path = path.clone();
            zgui::task::background(async move { client.format(&path, tab_size, spaces).await })
                .await
        };
        match edits {
            Ok(edits) if edits.is_empty() => workspace.say("nothing to format"),
            Ok(edits) => {
                // Applied back to front, because every edit's range is against the text as it was
                // and applying one moves everything after it.
                let mut replacements: Vec<(std::ops::Range<usize>, String)> =
                    handle.query(|snapshot| {
                        edits
                            .iter()
                            .map(|edit| {
                                (
                                    zdt_lsp::convert::range_of(
                                        snapshot.rope(),
                                        edit.range,
                                        encoding,
                                    ),
                                    edit.new_text.clone(),
                                )
                            })
                            .collect()
                    });
                replacements.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
                handle.command(zgui_editor::Command::ReplaceRanges(replacements));
                workspace.say("formatted");
            }
            Err(error) => workspace.complain(error.to_string()),
        }
    });
}

/// `<Leader>lF` in visual mode.
pub(super) fn format_selection(
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
    let encoding = encoding_for(language, &path);
    let (tab_size, spaces) = zgui::reactive::use_local_context::<crate::settings::Settings>()
        .map_or((4, true), |settings| {
            settings.with_untracked(|config| (config.editor.tab_size, config.editor.expand_tab))
        });

    let (range, empty) = handle.query(|snapshot| {
        let selection = snapshot.selections().primary();
        let (from, to) = (
            selection.anchor.min(selection.head),
            selection.anchor.max(selection.head),
        );
        (
            zdt_lsp::convert::lsp_range(snapshot.rope(), from..to, encoding),
            from == to,
        )
    });
    if empty {
        // Nothing selected means the whole file, which is what `=` with no motion means in vim.
        format(workspace, language, Some(&handle));
        return;
    }

    let workspace = workspace.clone();
    zdt_view::detached(async move {
        let edits = {
            let path = path.clone();
            zgui::task::background(async move {
                client.format_range(&path, range, tab_size, spaces).await
            })
            .await
        };
        match edits {
            Ok(edits) if edits.is_empty() => workspace.say("nothing to format"),
            Ok(edits) => {
                let mut replacements: Vec<(std::ops::Range<usize>, String)> =
                    handle.query(|snapshot| {
                        edits
                            .iter()
                            .map(|edit| {
                                (
                                    zdt_lsp::convert::range_of(
                                        snapshot.rope(),
                                        edit.range,
                                        encoding,
                                    ),
                                    edit.new_text.clone(),
                                )
                            })
                            .collect()
                    });
                replacements.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
                handle.command(zgui_editor::Command::ReplaceRanges(replacements));
                workspace.say("formatted");
            }
            Err(error) => workspace.complain(error.to_string()),
        }
    });
}
