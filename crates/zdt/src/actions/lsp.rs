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
        "definition" | "declaration" | "type_definition" | "implementation" => {
            go_to_definition(workspace, &language, handle);
        }
        "references" => references(workspace, &language, handle),
        "hover" => hover(workspace, &language, handle),
        "format" => format(workspace, &language, handle),
        "line_diagnostics" => line_diagnostics(workspace, &language, handle),
        "info" => info(workspace, &language),
        "restart" => {
            language.stop_all();
            if let Some(buffer) = workspace.current_buffer() {
                language.opened(buffer.id);
            }
            workspace.say("language servers restarted");
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

/// `gd` and its neighbours.
///
/// One answer opens it; several open a picker, because choosing between them is exactly what a
/// picker is for.
fn go_to_definition(workspace: &Workspace, language: &Language, handle: Option<&EditorHandle>) {
    let Some((handle, path)) = editing(workspace, handle) else {
        return;
    };
    let Some(mut client) = client(workspace, language, &path) else {
        return;
    };
    let position = caret_position(&handle, language, &path);

    let workspace = workspace.clone();
    crate::task::detached(async move {
        let found = {
            let path = path.clone();
            zgui::task::background(async move { client.definition(&path, position).await }).await
        };
        match found {
            Ok(locations) if locations.is_empty() => workspace.say("no definition"),
            Ok(locations) => open_location(&workspace, &locations[0]),
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
            Ok(locations) => workspace.say(format!("{} references", locations.len())),
            Err(error) => workspace.complain(error.to_string()),
        }
    });
}

/// `K`.
///
/// A panel anchored to the caret. The answer is markdown; what a hover holds is a signature and a
/// sentence, so the fences come out and the rest is shown as written.
fn hover(workspace: &Workspace, language: &Language, handle: Option<&EditorHandle>) {
    let Some((handle, path)) = editing(workspace, handle) else {
        return;
    };
    let Some(mut client) = client(workspace, language, &path) else {
        return;
    };
    let position = caret_position(&handle, language, &path);

    let workspace = workspace.clone();
    crate::task::detached(async move {
        let found = {
            let path = path.clone();
            zgui::task::background(async move { client.hover(&path, position).await }).await
        };
        match found {
            Ok(Some(found)) => {
                let panel = zgui::reactive::use_local_context::<crate::ui::hover::Hover>();
                // Where the caret is *now*: the answer took a round trip, and anchoring it to
                // where the caret was would put the panel somewhere nothing is.
                let at = handle.query(|snapshot| snapshot.selections().primary().head);
                match (panel, handle.point_for_byte(at)) {
                    (Some(panel), Some(rect)) => panel.show(&hover_text(&found), rect),
                    // Off screen, or no panel: the first line in the status bar still says
                    // something, which beats a key that appears to do nothing.
                    _ => workspace.say(one_line(&hover_text(&found))),
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
    match found.first() {
        Some(one) => workspace.say(one_line(&one.message)),
        None => workspace.say("nothing on this line"),
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

/// A hover's contents, as text.
fn hover_text(hover: &lsp_types::Hover) -> String {
    use lsp_types::{HoverContents, MarkedString};

    match &hover.contents {
        HoverContents::Scalar(MarkedString::String(text)) => text.clone(),
        HoverContents::Scalar(MarkedString::LanguageString(block)) => block.value.clone(),
        HoverContents::Markup(markup) => markup.value.clone(),
        HoverContents::Array(parts) => parts
            .iter()
            .map(|part| match part {
                MarkedString::String(text) => text.clone(),
                MarkedString::LanguageString(block) => block.value.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
    }
}

/// The first line worth reading out of a block of text.
///
/// Markdown fences and blank lines are skipped rather than shown: a status line that says "```" is
/// a status line that has said nothing.
fn one_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("```"))
        .unwrap_or("")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fenced_block_reads_as_its_first_real_line() {
        let text = "```rust\nfn main()\n```\nDoes the thing.";
        assert_eq!(one_line(text), "fn main()");
    }

    #[test]
    fn blank_lines_are_skipped() {
        assert_eq!(one_line("\n\n  the answer  \nmore"), "the answer");
    }

    #[test]
    fn nothing_readable_is_an_empty_line_rather_than_a_fence() {
        assert_eq!(one_line("```\n```"), "");
        assert_eq!(one_line(""), "");
    }

    #[test]
    fn every_shape_of_hover_reads() {
        use lsp_types::{HoverContents, MarkedString, MarkupContent, MarkupKind};

        let scalar = lsp_types::Hover {
            contents: HoverContents::Scalar(MarkedString::String("plain".to_owned())),
            range: None,
        };
        assert_eq!(hover_text(&scalar), "plain");

        // The markdown comes through as written: the panel strips its own fences, because it is
        // the thing that knows it is a monospace box.
        let markup = lsp_types::Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: "```rust\nlet x: i32\n```".to_owned(),
            }),
            range: None,
        };
        assert_eq!(crate::ui::hover::tidy(&hover_text(&markup)), "let x: i32");

        let array = lsp_types::Hover {
            contents: HoverContents::Array(vec![
                MarkedString::String(String::new()),
                MarkedString::String("second".to_owned()),
            ]),
            range: None,
        };
        assert_eq!(crate::ui::hover::tidy(&hover_text(&array)), "second");
    }
}
