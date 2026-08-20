//! Picking a symbol out of a file, a project, or the diagnostics.

use crate::actions::lsp::shared::*;
use crate::language::Language;
use crate::workspace::Workspace;
use zgui_editor::EditorHandle;

/// `<Leader>lo`.
///
/// Everything the file declares, as a picker. The hierarchy is flattened with an indent, because
/// a picker filters and a filtered tree stops being a tree. The indent still shows what is inside
/// what while nothing has been typed.
pub(super) fn outline(workspace: &Workspace, language: &Language, handle: Option<&EditorHandle>) {
    let Some((_, path)) = editing(workspace, handle) else {
        return;
    };
    let Some(mut client) = client(workspace, language, &path) else {
        return;
    };
    let picker = zgui::reactive::use_local_context::<crate::picker::Picker>();

    let workspace = workspace.clone();
    zdt_view::detached(async move {
        let found = {
            let path = path.clone();
            zgui::task::background(async move { client.document_symbols(&path).await }).await
        };
        match found {
            Ok(Some(answer)) => {
                let rows = outline_rows(&answer, &path);
                if rows.is_empty() {
                    workspace.say("nothing in this file");
                    return;
                }
                if let Some(picker) = picker {
                    picker.open(crate::picker::Source::Given {
                        title: "Symbols",
                        rows,
                        typed: None,
                    });
                }
            }
            Ok(None) => workspace.say("nothing in this file"),
            Err(error) => workspace.complain(error.to_string()),
        }
    });
}

/// The two shapes a document-symbol answer comes in, as rows.
pub(super) fn outline_rows(
    answer: &lsp_types::DocumentSymbolResponse,
    path: &std::path::Path,
) -> Vec<crate::picker::Row> {
    use crate::picker::source::symbol_mark;
    use crate::picker::{Row, Target};

    fn nested(
        symbols: &[lsp_types::DocumentSymbol],
        path: &std::path::Path,
        depth: usize,
        out: &mut Vec<Row>,
    ) {
        for symbol in symbols {
            let (glyph, tint) = symbol_mark(symbol.kind);
            let line = u64::from(symbol.selection_range.start.line) + 1;
            out.push(Row {
                // Two spaces per level: enough that the nesting is visible, little enough that
                // a symbol six deep is still readable at the left of the panel.
                label: format!("{}{}", "  ".repeat(depth), symbol.name),
                detail: symbol.detail.clone().unwrap_or_default(),
                matched: Vec::new(),
                glyph: Some(glyph),
                tint: Some(tint),
                target: Target::File {
                    path: path.to_path_buf(),
                    line: Some(line),
                    matched: None,
                },
            });
            if let Some(children) = symbol.children.as_ref() {
                nested(children, path, depth + 1, out);
            }
        }
    }

    match answer {
        lsp_types::DocumentSymbolResponse::Nested(symbols) => {
            let mut out = Vec::new();
            nested(symbols, path, 0, &mut out);
            out
        }
        lsp_types::DocumentSymbolResponse::Flat(symbols) => symbols
            .iter()
            .map(|symbol| {
                let (glyph, tint) = symbol_mark(symbol.kind);
                let line = u64::from(symbol.location.range.start.line) + 1;
                Row {
                    label: symbol.name.clone(),
                    detail: symbol.container_name.clone().unwrap_or_default(),
                    matched: Vec::new(),
                    glyph: Some(glyph),
                    tint: Some(tint),
                    target: Target::File {
                        path: path.to_path_buf(),
                        line: Some(line),
                        matched: None,
                    },
                }
            })
            .collect(),
    }
}

/// `<Leader>lS`.
pub(super) fn workspace_symbols(workspace: &Workspace) {
    let Some(picker) = zgui::reactive::use_local_context::<crate::picker::Picker>() else {
        return;
    };
    let _ = workspace;
    picker.open(crate::picker::Source::WorkspaceSymbols);
}

/// `<Leader>xx`: everything wrong in the project, as a picker.
pub(super) fn diagnostics_picker(workspace: &Workspace, language: &Language) {
    use crate::picker::{Row, Target};

    let Some(picker) = zgui::reactive::use_local_context::<crate::picker::Picker>() else {
        return;
    };
    let root = workspace.project().root().to_path_buf();

    let mut rows: Vec<Row> = Vec::new();
    for path in language.files() {
        let relative = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .to_string_lossy()
            .into_owned();
        for one in language.diagnostics(&path) {
            let line = u64::from(one.range.start.line) + 1;
            let severity = one.severity.unwrap_or(lsp_types::DiagnosticSeverity::ERROR);
            rows.push(Row {
                label: one.message.lines().next().unwrap_or("").to_owned(),
                detail: format!("{relative}:{line}"),
                matched: Vec::new(),
                glyph: Some(crate::language::diagnostics::glyph(Some(severity))),
                tint: Some(crate::language::diagnostics::tint(Some(severity))),
                target: Target::File {
                    path: path.clone(),
                    line: Some(line),
                    matched: None,
                },
            });
        }
    }

    if rows.is_empty() {
        workspace.say("nothing wrong anywhere");
        return;
    }
    picker.open(crate::picker::Source::Given {
        title: "Diagnostics",
        rows,
        typed: None,
    });
}

/// Shows several places to go, as a picker.
pub(super) fn show_locations(
    workspace: &Workspace,
    picker: Option<crate::picker::Picker>,
    title: &'static str,
    locations: &[lsp_types::Location],
) {
    let Some(picker) = picker else {
        workspace.say(format!("{} places", locations.len()));
        return;
    };
    let root = workspace.project().root().to_path_buf();
    let rows = crate::picker::location_rows(locations, &root);
    if rows.is_empty() {
        workspace.say("nowhere a file can be opened");
        return;
    }
    picker.open(crate::picker::Source::Given {
        title,
        rows,
        typed: None,
    });
}
