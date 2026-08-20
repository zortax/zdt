//! The fixes and refactors a server offers.

use crate::actions::lsp::shared::*;
use crate::language::Language;
use crate::workspace::Workspace;
use zgui_editor::EditorHandle;

/// `<Leader>la`.
///
/// What the server could do about where the caret is, as a picker. The diagnostics on the line go
/// with the request, because that is how a server knows which quick fixes to offer.
pub(super) fn code_action(
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

    // The selection when there is one, and the caret's line when there is not: `<Leader>la` over a
    // block means "do something about this block".
    let range = handle.query(|snapshot| {
        let selection = snapshot.selections().primary();
        let (from, to) = (
            selection.anchor.min(selection.head),
            selection.anchor.max(selection.head),
        );
        zdt_lsp::convert::lsp_range(snapshot.rope(), from..to, encoding)
    });
    let line = handle.query(|snapshot| {
        let caret = snapshot.selections().primary().head;
        snapshot.rope().byte_to_line(caret) as u32
    });
    let diagnostics = language.on_line(&path, line);
    // Taken now, while there is a scope to take it from.
    let picker = zgui::reactive::use_local_context::<crate::picker::Picker>();

    let workspace = workspace.clone();
    zdt_view::detached(async move {
        let found = {
            let path = path.clone();
            zgui::task::background(
                async move { client.code_action(&path, range, diagnostics).await },
            )
            .await
        };
        match found {
            Ok(actions) if actions.is_empty() => workspace.say("nothing to do here"),
            Ok(actions) => offer_actions(&workspace, picker, actions, encoding),
            Err(error) => workspace.complain(error.to_string()),
        }
    });
}

/// Puts a list of code actions in the picker, each row carrying what it does.
pub(super) fn offer_actions(
    workspace: &Workspace,
    picker: Option<crate::picker::Picker>,
    actions: Vec<lsp_types::CodeActionOrCommand>,
    encoding: zdt_lsp::Encoding,
) {
    use crate::picker::{Deed, Row, Source, Target};

    let Some(picker) = picker else {
        return;
    };

    let rows: Vec<Row> = actions
        .into_iter()
        .map(|offered| {
            let (label, detail) = match &offered {
                lsp_types::CodeActionOrCommand::Command(command) => {
                    (command.title.clone(), String::new())
                }
                lsp_types::CodeActionOrCommand::CodeAction(action) => (
                    action.title.clone(),
                    action
                        .kind
                        .as_ref()
                        .map(|kind| kind.as_str().to_owned())
                        .unwrap_or_default(),
                ),
            };
            let workspace = workspace.clone();
            Row::plain(
                label,
                Target::Run(Deed::new(move || {
                    run_action(&workspace, offered.clone(), encoding);
                })),
            )
            .with_detail(detail)
            .with_glyph("\u{f0335}", "zdt-diagnostic-hint")
        })
        .collect();

    picker.open(Source::Given {
        title: "Code actions",
        rows,
        typed: None,
    });
}

/// Does what one code action says.
///
/// Two things it can be, and it can be both: an edit to apply, and a command to run. The edit goes
/// first, because a command that acts on the edited text has to see it.
pub(super) fn run_action(
    workspace: &Workspace,
    offered: lsp_types::CodeActionOrCommand,
    encoding: zdt_lsp::Encoding,
) {
    let notify = crate::notify::use_notify();
    let Some(language) = zgui::reactive::use_local_context::<Language>() else {
        return;
    };
    let Some(path) = language.current_path() else {
        return;
    };
    let Some(mut client) = language.client_for(&path) else {
        return;
    };

    let workspace = workspace.clone();
    zdt_view::detached(async move {
        let action = match offered {
            lsp_types::CodeActionOrCommand::Command(command) => {
                let found =
                    zgui::task::background(async move { client.execute_command(command).await })
                        .await;
                if let Err(error) = found
                    && let Some(notify) = notify.as_ref()
                {
                    notify.fail("code action", Some(error.to_string()));
                }
                return;
            }
            lsp_types::CodeActionOrCommand::CodeAction(action) => action,
        };

        // An action can arrive with a title and nothing else: servers compute the edit only for
        // the one that is chosen.
        let resolved = {
            let mut client = client.clone();
            zgui::task::background(async move { client.resolve_code_action(action).await }).await
        };
        let Ok(resolved) = resolved else {
            if let Some(notify) = notify.as_ref() {
                notify.fail(
                    "code action",
                    Some("the server could not work it out".to_owned()),
                );
            }
            return;
        };

        if let Some(edit) = resolved.edit {
            crate::actions::edit::apply(&workspace, notify.as_ref(), edit, encoding);
        }
        if let Some(command) = resolved.command {
            let found =
                zgui::task::background(async move { client.execute_command(command).await }).await;
            if let Err(error) = found
                && let Some(notify) = notify.as_ref()
            {
                notify.fail("code action", Some(error.to_string()));
            }
        }
    });
}
