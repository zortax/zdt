//! The signature of the call the caret is inside.

use crate::actions::lsp::shared::*;
use crate::language::Language;
use crate::workspace::Workspace;
use zgui_editor::EditorHandle;

/// `<Leader>ls` and `<C-s>` in insert mode.
///
/// A panel under the caret saying what the call being typed takes, with the parameter being typed
/// picked out. The hover panel draws it, because both are documentation anchored to the caret. Two
/// panels that looked slightly different would be worse than one.
pub(super) fn signature_help(
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
    let panel = zgui::reactive::use_local_context::<crate::hover::Hover>();

    let workspace = workspace.clone();
    zdt_view::detached(async move {
        let found = {
            let path = path.clone();
            zgui::task::background(async move { client.signature_help(&path, position).await })
                .await
        };
        match found {
            Ok(Some(help)) if !help.signatures.is_empty() => {
                let markdown = signature_markdown(&help);
                let at = handle.query(|snapshot| snapshot.selections().primary().head);
                match (panel, handle.point_for_byte(at)) {
                    (Some(panel), Some(rect)) => panel.show_markdown(&markdown, rect),
                    _ => workspace.say(crate::hover::one_line(&markdown)),
                }
            }
            Ok(_) => workspace.say("no signature here"),
            Err(error) => workspace.complain(error.to_string()),
        }
    });
}

/// A signature, as markdown, with the parameter being typed picked out.
///
/// The active parameter is bold. This goes through the markdown renderer, and bold is the
/// emphasis that renderer has. Which parameter is active is the whole value of the panel. A
/// signature whose shape somebody can already see is worth much less than knowing which slot the
/// caret is in.
pub(super) fn signature_markdown(help: &lsp_types::SignatureHelp) -> String {
    let index = help.active_signature.unwrap_or(0) as usize;
    let Some(signature) = help
        .signatures
        .get(index)
        .or_else(|| help.signatures.first())
    else {
        return String::new();
    };

    let active = signature
        .active_parameter
        .or(help.active_parameter)
        .map(|at| at as usize);
    let label = &signature.label;

    let mut out = String::new();
    // The parameter's own span in the label, when the server gave offsets. A server that gave a
    // string instead is one whose parameter cannot be located in the label reliably, so its
    // signature is shown whole.
    let span = active
        .and_then(|at| signature.parameters.as_ref()?.get(at).cloned())
        .and_then(|parameter| match parameter.label {
            lsp_types::ParameterLabel::LabelOffsets([from, to]) => {
                Some((from as usize, to as usize))
            }
            lsp_types::ParameterLabel::Simple(_) => None,
        })
        .filter(|(from, to)| *to <= label.len() && from < to);

    match span {
        Some((from, to)) => {
            out.push_str("```\n");
            out.push_str(label);
            out.push_str("\n```\n\n");
            out.push_str("**");
            out.push_str(&label[from..to]);
            out.push_str("**");
        }
        None => {
            out.push_str("```\n");
            out.push_str(label);
            out.push_str("\n```");
        }
    }

    if let Some(documentation) = signature.documentation.as_ref() {
        let text = match documentation {
            lsp_types::Documentation::String(text) => text.clone(),
            lsp_types::Documentation::MarkupContent(markup) => markup.value.clone(),
        };
        if !text.trim().is_empty() {
            out.push_str("\n\n");
            out.push_str(&text);
        }
    }
    out
}
