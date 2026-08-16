//! The pieces the rest of this share.

use crate::language::Language;
use crate::workspace::Workspace;
use std::path::PathBuf;
use zgui_editor::EditorHandle;

// ---- The pieces the above share ---------------------------------------------------------------

/// The editor and the file being edited, when there is a file.
pub(super) fn editing(
    workspace: &Workspace,
    handle: Option<&EditorHandle>,
) -> Option<(EditorHandle, PathBuf)> {
    let handle = handle.cloned()?;
    let path = workspace.current_buffer().and_then(|buffer| buffer.path)?;
    Some((handle, path))
}

/// The client answering for a file, complaining when there is not one.
pub(super) fn client(
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
pub(super) fn encoding_for(language: &Language, path: &std::path::Path) -> zdt_lsp::Encoding {
    language
        .client_for(path)
        .map(|client| client.encoding)
        .unwrap_or_default()
}

/// Where the caret is, as the protocol would say it.
pub(super) fn caret_position(
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
pub(super) fn open_location(workspace: &Workspace, location: &lsp_types::Location) {
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
pub(super) fn one_line(text: &str) -> String {
    crate::hover::one_line(text)
}
