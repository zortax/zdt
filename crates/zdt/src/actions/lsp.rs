//! What the language keys do.
//!
//! Every one of these is the same shape: read where the caret is *now*, take a client to a worker,
//! await an answer, and come back to the interface thread to do something with it. The reading
//! happens first and on this thread, because by the time an answer arrives the caret may have
//! moved — and a definition looked up for where the caret is now is the one that was asked for.

use std::path::PathBuf;

use zgui_editor::EditorHandle;

use crate::language::Language;
use crate::workspace::Workspace;

/// Carries out one `lsp.*` action.
pub fn run(workspace: &Workspace, leaf: &str, handle: Option<&EditorHandle>) {
    let Some(language) = zgui::reactive::use_local_context::<Language>() else {
        return;
    };

    match leaf {
        // Four questions, not one. `gd` and `gy` mean different things in every language that has
        // both, and answering all four with `definition` was a placeholder rather than a design.
        "definition" | "declaration" | "type_definition" | "implementation" => {
            go_to(workspace, &language, handle, leaf);
        }
        "references" => references(workspace, &language, handle),
        "hover" => hover(workspace, &language, handle),
        "signature_help" => signature_help(workspace, &language, handle),
        "rename" => rename(workspace, &language, handle),
        "code_action" => code_action(workspace, &language, handle),
        "outline" | "symbols" => outline(workspace, &language, handle),
        "workspace_symbols" => workspace_symbols(workspace),
        "format" => format(workspace, &language, handle),
        "format_selection" => format_selection(workspace, &language, handle),
        "line_diagnostics" => line_diagnostics(workspace, &language, handle),
        "diagnostics" => diagnostics_picker(workspace, &language),
        "info" => info(workspace, &language),
        "restart" => {
            language.stop_all();
            if let Some(buffer) = workspace.current_buffer() {
                language.opened(buffer.id);
            }
            crate::notify::say("language servers restarted");
        }
        other => workspace.say(format!("lsp.{other} is not built yet")),
    }
}

/// `]e` and `[e`.
pub fn diagnostic(workspace: &Workspace, leaf: &str, handle: Option<&EditorHandle>) {
    let Some(language) = zgui::reactive::use_local_context::<Language>() else {
        return;
    };
    let Some((handle, path)) = editing(workspace, handle) else {
        return;
    };

    let line = handle.query(|snapshot| {
        let caret = snapshot.selections().primary().head;
        snapshot.rope().byte_to_line(caret) as u32
    });

    let found = match leaf {
        "next" => language.after(&path, line),
        "previous" => language.before(&path, line),
        other => {
            workspace.say(format!("diagnostic.{other} is not built yet"));
            return;
        }
    };

    let Some(found) = found else {
        workspace.say("nothing to fix");
        return;
    };

    let encoding = encoding_for(&language, &path);
    let at = handle
        .query(|snapshot| zdt_lsp::convert::byte_of(snapshot.rope(), found.range.start, encoding));
    handle.command(zgui_editor::Command::SetSelections {
        selections: vec![zgui_editor::Selection::caret(at)],
        primary: 0,
    });
    handle.command(zgui_editor::Command::Scroll(
        zgui_editor::ScrollCmd::CursorCenter,
    ));
    workspace.say(one_line(&found.message));
}

/// `gd`, `gD`, `gy` and `gI`.
///
/// One answer opens it; several open a picker, because choosing between them is exactly what a
/// picker is for. Which question is asked is the action's own leaf — the four are different
/// questions in every language that has all of them, and a `gy` that answered `gd` was a
/// placeholder.
fn go_to(workspace: &Workspace, language: &Language, handle: Option<&EditorHandle>, which: &str) {
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
    crate::task::detached(async move {
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
fn references(workspace: &Workspace, language: &Language, handle: Option<&EditorHandle>) {
    let Some((handle, path)) = editing(workspace, handle) else {
        return;
    };
    let Some(mut client) = client(workspace, language, &path) else {
        return;
    };
    let position = caret_position(&handle, language, &path);
    let picker = zgui::reactive::use_local_context::<crate::picker::Picker>();

    let workspace = workspace.clone();
    crate::task::detached(async move {
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

/// `<Leader>ls` and `<C-s>` in insert mode.
///
/// A panel under the caret saying what the call being typed takes, with the parameter being typed
/// picked out. The hover panel draws it, because it is the same shape of thing — documentation
/// anchored to the caret — and two panels that looked slightly different would be worse than one.
fn signature_help(workspace: &Workspace, language: &Language, handle: Option<&EditorHandle>) {
    let Some((handle, path)) = editing(workspace, handle) else {
        return;
    };
    let Some(mut client) = client(workspace, language, &path) else {
        return;
    };
    let position = caret_position(&handle, language, &path);
    let panel = zgui::reactive::use_local_context::<crate::ui::hover::Hover>();

    let workspace = workspace.clone();
    crate::task::detached(async move {
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
                    _ => workspace.say(crate::ui::hover::one_line(&markdown)),
                }
            }
            Ok(_) => workspace.say("no signature here"),
            Err(error) => workspace.complain(error.to_string()),
        }
    });
}

/// A signature, as markdown, with the parameter being typed picked out.
///
/// The active parameter in bold rather than in a colour: this goes through the markdown renderer,
/// and bold is the emphasis that renderer has. Which one is active is the whole value of the
/// panel — a signature somebody can already see the shape of is worth much less than knowing
/// which slot the caret is in.
fn signature_markdown(help: &lsp_types::SignatureHelp) -> String {
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

/// `<Leader>lr`.
///
/// Asks the server what exactly would be renamed, opens a box over it, and applies whatever comes
/// back. Asking first is what lets a key pressed on a keyword say "no" before somebody has typed a
/// new name for it.
fn rename(workspace: &Workspace, language: &Language, handle: Option<&EditorHandle>) {
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
    let box_of = zgui::reactive::use_local_context::<crate::ui::rename::Rename>();

    let workspace = workspace.clone();
    crate::task::detached(async move {
        let asked = {
            let path = path.clone();
            zgui::task::background(async move { client.prepare_rename(&path, position).await })
                .await
        };

        // What the server said would be renamed, or the word under the caret when it said nothing.
        // A server that refuses outright is one saying this cannot be renamed, which is worth
        // hearing before a name has been typed rather than after.
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
/// Called by the rename box when it is accepted, rather than by a key: the box is the thing that
/// knows what was typed.
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
    crate::task::detached(async move {
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

/// `<Leader>la`.
///
/// What the server could do about where the caret is, as a picker. The diagnostics on the line go
/// with the request, because that is how a server knows which quick fixes to offer.
fn code_action(workspace: &Workspace, language: &Language, handle: Option<&EditorHandle>) {
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
    crate::task::detached(async move {
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
fn offer_actions(
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
    });
}

/// Does what one code action says.
///
/// Two things it can be, and it can be both: an edit to apply, and a command to run. The edit goes
/// first, because a command that acts on the edited text has to see it.
fn run_action(
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
    crate::task::detached(async move {
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

/// `<Leader>lo`.
///
/// Everything the file declares, as a picker. The hierarchy is flattened with an indent, because a
/// picker filters and a filtered tree is not a tree — but the indent keeps what is inside what
/// readable while nothing has been typed.
fn outline(workspace: &Workspace, language: &Language, handle: Option<&EditorHandle>) {
    let Some((_, path)) = editing(workspace, handle) else {
        return;
    };
    let Some(mut client) = client(workspace, language, &path) else {
        return;
    };
    let picker = zgui::reactive::use_local_context::<crate::picker::Picker>();

    let workspace = workspace.clone();
    crate::task::detached(async move {
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
                    });
                }
            }
            Ok(None) => workspace.say("nothing in this file"),
            Err(error) => workspace.complain(error.to_string()),
        }
    });
}

/// The two shapes a document-symbol answer comes in, as rows.
fn outline_rows(
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
fn workspace_symbols(workspace: &Workspace) {
    let Some(picker) = zgui::reactive::use_local_context::<crate::picker::Picker>() else {
        return;
    };
    let _ = workspace;
    picker.open(crate::picker::Source::WorkspaceSymbols);
}

/// `<Leader>xx`: everything wrong in the project, as a picker.
fn diagnostics_picker(workspace: &Workspace, language: &Language) {
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
                glyph: Some(crate::ui::diagnostics::glyph(Some(severity))),
                tint: Some(crate::ui::diagnostics::tint(Some(severity))),
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
    });
}

/// Shows several places to go, as a picker.
fn show_locations(
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
    picker.open(crate::picker::Source::Given { title, rows });
}

/// `K`.
///
/// A panel anchored to the caret, holding the answer drawn as the markdown it is.
///
/// Pressed a second time while the panel is up it gives the panel the keyboard instead of asking
/// again: the second press means "I want to read this", and what somebody reading a panel needs is
/// to be able to scroll it. The request is not repeated, because the answer has not changed.
fn hover(workspace: &Workspace, language: &Language, handle: Option<&EditorHandle>) {
    if let Some(panel) = zgui::reactive::use_local_context::<crate::ui::hover::Hover>()
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
    // Taken *now*, while there is a scope to take it from: a context looked up after an await is
    // not there, and the panel would silently never open — see `tests/context.rs`.
    let panel = zgui::reactive::use_local_context::<crate::ui::hover::Hover>();

    let workspace = workspace.clone();
    crate::task::detached(async move {
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
                    _ => workspace.say(crate::ui::hover::one_line(&crate::ui::hover::markdown_of(
                        &found.contents,
                    ))),
                }
            }
            Ok(None) => workspace.say("nothing here"),
            Err(error) => workspace.complain(error.to_string()),
        }
    });
}

/// `<Leader>lf`.
fn format(workspace: &Workspace, language: &Language, handle: Option<&EditorHandle>) {
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
    crate::task::detached(async move {
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
fn format_selection(workspace: &Workspace, language: &Language, handle: Option<&EditorHandle>) {
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
    crate::task::detached(async move {
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

/// `<Leader>ld` and `gl`.
fn line_diagnostics(workspace: &Workspace, language: &Language, handle: Option<&EditorHandle>) {
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

    // In the panel rather than the status line: a diagnostic is a paragraph and sometimes several,
    // and a line that shows the first sentence of a borrow-checker error has shown nothing. The
    // panel is the same one `K` opens, so it scrolls with the same keys.
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

    let panel = zgui::reactive::use_local_context::<crate::ui::hover::Hover>();
    let at = handle.query(|snapshot| snapshot.selections().primary().head);
    match (panel, handle.point_for_byte(at)) {
        (Some(panel), Some(rect)) => panel.show_markdown(&markdown, rect),
        _ => workspace.say(one_line(&found[0].message)),
    }
}

/// What a server calls a diagnostic, when it calls it anything.
fn code_of(one: &lsp_types::Diagnostic) -> Option<String> {
    match one.code.as_ref()? {
        lsp_types::NumberOrString::Number(number) => Some(number.to_string()),
        lsp_types::NumberOrString::String(text) => Some(text.clone()),
    }
}

/// `<Leader>li`.
fn info(workspace: &Workspace, language: &Language) {
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

// ---- The pieces the above share ---------------------------------------------------------------

/// The editor and the file being edited, when there is a file.
fn editing(
    workspace: &Workspace,
    handle: Option<&EditorHandle>,
) -> Option<(EditorHandle, PathBuf)> {
    let handle = handle.cloned()?;
    let path = workspace.current_buffer().and_then(|buffer| buffer.path)?;
    Some((handle, path))
}

/// The client answering for a file, complaining when there is not one.
fn client(
    workspace: &Workspace,
    language: &Language,
    path: &std::path::Path,
) -> Option<zdt_lsp::Client> {
    match language.client_for(path) {
        Some(client) => Some(client),
        None => {
            workspace.say("no language server for this file");
            None
        }
    }
}

/// How the server answering for a file counts characters.
fn encoding_for(language: &Language, path: &std::path::Path) -> zdt_lsp::Encoding {
    language
        .client_for(path)
        .map(|client| client.encoding)
        .unwrap_or_default()
}

/// Where the caret is, as the protocol would say it.
fn caret_position(
    handle: &EditorHandle,
    language: &Language,
    path: &std::path::Path,
) -> lsp_types::Position {
    let encoding = encoding_for(language, path);
    handle.query(|snapshot| {
        let caret = snapshot.selections().primary().head;
        zdt_lsp::convert::position_of(snapshot.rope(), caret, encoding)
    })
}

/// Opens what a location names, at its line.
fn open_location(workspace: &Workspace, location: &lsp_types::Location) {
    let Some(path) = zdt_lsp::convert::path_of(&location.uri) else {
        workspace.complain("the server named somewhere that is not a file");
        return;
    };
    crate::files::open_at(
        workspace,
        path,
        Some(u64::from(location.range.start.line) + 1),
    );
}

/// The first line worth reading out of a block of text, for the status line.
fn one_line(text: &str) -> String {
    crate::ui::hover::one_line(text)
}
