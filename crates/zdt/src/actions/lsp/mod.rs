//! What the language keys do.
//!
//! Every one of these has the same shape: read where the caret is *now*, take a client to a
//! worker, await an answer, and come back to the interface thread to do something with it. The
//! reading happens first and on this thread. By the time an answer arrives the caret may have
//! moved, and the definition that was asked for is the one at the caret's old place.

mod code_action;
mod format;
mod goto;
mod hover;
mod info;
mod rename;
mod shared;
mod signature;
mod symbols;

// The rename prompt reaches for this once the new name has been typed.
pub use crate::actions::lsp::rename::rename_to;

use crate::actions::lsp::shared::{editing, encoding_for, one_line};

use zgui_editor::EditorHandle;

use crate::language::Language;
use crate::workspace::Workspace;

/// Carries out one `lsp.*` action.
pub fn run(workspace: &Workspace, leaf: &str, handle: Option<&EditorHandle>) {
    let Some(language) = zgui::reactive::use_local_context::<Language>() else {
        return;
    };

    match leaf {
        // Four questions, and not one. `gd` and `gy` mean different things in every language
        // that has both. Answering all four with `definition` was a placeholder.
        "definition" | "declaration" | "type_definition" | "implementation" => {
            self::goto::go_to(workspace, &language, handle, leaf);
        }
        "references" => self::goto::references(workspace, &language, handle),
        "hover" => self::hover::hover(workspace, &language, handle),
        "signature_help" => self::signature::signature_help(workspace, &language, handle),
        "rename" => self::rename::rename(workspace, &language, handle),
        "code_action" => self::code_action::code_action(workspace, &language, handle),
        "outline" | "symbols" => self::symbols::outline(workspace, &language, handle),
        "workspace_symbols" => self::symbols::workspace_symbols(workspace),
        "format" => self::format::format(workspace, &language, handle),
        "format_selection" => self::format::format_selection(workspace, &language, handle),
        "line_diagnostics" => self::info::line_diagnostics(workspace, &language, handle),
        "diagnostics" => self::symbols::diagnostics_picker(workspace, &language),
        "info" => self::info::info(workspace, &language),
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
