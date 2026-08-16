use std::path::Path;

use async_lsp::LanguageServer;
use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, TextDocumentContentChangeEvent, TextDocumentIdentifier,
    TextDocumentItem, VersionedTextDocumentIdentifier,
};

use super::Client;

impl Client {
    /// Tells the server a file is open, with the text the editor has.
    pub fn open(&mut self, path: &Path, language: &str, version: i32, text: String) {
        let Some(uri) = crate::convert::uri_of(path) else {
            return;
        };
        let _ = self.socket.did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri,
                language_id: language.to_owned(),
                version,
                text,
            },
        });
    }

    /// Tells it what changed.
    ///
    /// `changes` are incremental when the server asked for incremental sync, and whole-text when
    /// it did not. The caller decides, because only it knows what the editor reported.
    pub fn change(
        &mut self,
        path: &Path,
        version: i32,
        changes: Vec<TextDocumentContentChangeEvent>,
    ) {
        let Some(uri) = crate::convert::uri_of(path) else {
            return;
        };
        let _ = self.socket.did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier { uri, version },
            content_changes: changes,
        });
    }

    /// Tells it the file has been written.
    pub fn save(&mut self, path: &Path, text: Option<String>) {
        let Some(uri) = crate::convert::uri_of(path) else {
            return;
        };
        let _ = self.socket.did_save(DidSaveTextDocumentParams {
            text_document: TextDocumentIdentifier { uri },
            text,
        });
    }

    /// Tells it the file is closed, so it can forget its diagnostics.
    pub fn close(&mut self, path: &Path) {
        let Some(uri) = crate::convert::uri_of(path) else {
            return;
        };
        let _ = self.socket.did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri },
        });
    }
}
