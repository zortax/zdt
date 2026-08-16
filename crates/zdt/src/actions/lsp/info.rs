//! The diagnostics on the caret's line, and what the server is doing.

use crate::actions::lsp::shared::*;
use crate::language::Language;
use crate::workspace::Workspace;
use zgui_editor::EditorHandle;

/// `<Leader>ld` and `gl`.
pub(super) fn line_diagnostics(
    workspace: &Workspace,
    language: &Language,
    handle: Option<&EditorHandle>,
) {
    let Some((handle, path)) = editing(workspace, handle) else {
        return;
    };
    let line = handle.query(|snapshot| {
        let caret = snapshot.selections().primary().head;
        snapshot.rope().byte_to_line(caret) as u32
    });

    let found = language.on_line(&path, line);
    if found.is_empty() {
        workspace.say("nothing on this line");
        return;
    }

    // In the panel, and not the status line. A diagnostic is a paragraph and sometimes several,
    // and one line of a borrow-checker error says nothing. The panel is the same one `K` opens,
    // so it scrolls with the same keys.
    let markdown = found
        .iter()
        .map(|one| {
            let severity = match one.severity {
                Some(lsp_types::DiagnosticSeverity::WARNING) => "warning",
                Some(lsp_types::DiagnosticSeverity::INFORMATION) => "note",
                Some(lsp_types::DiagnosticSeverity::HINT) => "hint",
                _ => "error",
            };
            // Where it came from, when the server says: `rust-analyzer` names the lint, and the
            // name is half of what makes an error searchable.
            let source = match (one.source.as_deref(), code_of(one)) {
                (Some(from), Some(code)) => format!(" ({from}: {code})"),
                (Some(from), None) => format!(" ({from})"),
                (None, Some(code)) => format!(" ({code})"),
                (None, None) => String::new(),
            };
            format!("**{severity}**{source}\n\n{}", one.message)
        })
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");

    let panel = zgui::reactive::use_local_context::<crate::hover::Hover>();
    let at = handle.query(|snapshot| snapshot.selections().primary().head);
    match (panel, handle.point_for_byte(at)) {
        (Some(panel), Some(rect)) => panel.show_markdown(&markdown, rect),
        _ => workspace.say(one_line(&found[0].message)),
    }
}

/// What a server calls a diagnostic, when it calls it anything.
pub(super) fn code_of(one: &lsp_types::Diagnostic) -> Option<String> {
    match one.code.as_ref()? {
        lsp_types::NumberOrString::Number(number) => Some(number.to_string()),
        lsp_types::NumberOrString::String(text) => Some(text.clone()),
    }
}

/// `<Leader>li`.
pub(super) fn info(workspace: &Workspace, language: &Language) {
    let Some(path) = workspace.current_buffer().and_then(|buffer| buffer.path) else {
        workspace.say("no language servers");
        return;
    };
    let servers = language.servers_for(&path);
    if servers.is_empty() {
        workspace.say("no language servers for this file");
    } else {
        workspace.say(servers.join(", "));
    }
}
