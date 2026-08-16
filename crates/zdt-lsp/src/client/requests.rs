use std::path::Path;

use async_lsp::LanguageServer;
use lsp_types::{
    CompletionParams, DocumentFormattingParams, GotoDefinitionParams, HoverParams,
    PartialResultParams, Position, ReferenceContext, ReferenceParams, TextDocumentIdentifier,
    TextDocumentPositionParams, TextEdit, WorkDoneProgressParams,
};

use super::{Client, ClientError};

use crate::client::symbol::{Symbol, locations, symbols};

impl Client {
    /// What is at `position`, as documentation.
    ///
    /// # Errors
    ///
    /// If the server refuses or the connection has broken.
    pub async fn hover(
        &mut self,
        path: &Path,
        position: Position,
    ) -> Result<Option<lsp_types::Hover>, ClientError> {
        let params = HoverParams {
            text_document_position_params: self.at(path, position)?,
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        self.socket
            .hover(params)
            .await
            .map_err(|error| ClientError::Protocol(error.to_string()))
    }

    /// Where what is at `position` is defined.
    ///
    /// # Errors
    ///
    /// As [`Client::hover`].
    pub async fn definition(
        &mut self,
        path: &Path,
        position: Position,
    ) -> Result<Vec<lsp_types::Location>, ClientError> {
        let params = GotoDefinitionParams {
            text_document_position_params: self.at(path, position)?,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let answer = self
            .socket
            .definition(params)
            .await
            .map_err(|error| ClientError::Protocol(error.to_string()))?;
        Ok(locations(answer))
    }

    /// Where what is at `position` is *declared*, which in a language with headers is somewhere
    /// else.
    ///
    /// # Errors
    ///
    /// As [`Client::hover`].
    pub async fn declaration(
        &mut self,
        path: &Path,
        position: Position,
    ) -> Result<Vec<lsp_types::Location>, ClientError> {
        let params = lsp_types::request::GotoDeclarationParams {
            text_document_position_params: self.at(path, position)?,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let answer = self
            .socket
            .declaration(params)
            .await
            .map_err(|error| ClientError::Protocol(error.to_string()))?;
        Ok(locations(answer))
    }

    /// Where the *type* of what is at `position` is defined.
    ///
    /// # Errors
    ///
    /// As [`Client::hover`].
    pub async fn type_definition(
        &mut self,
        path: &Path,
        position: Position,
    ) -> Result<Vec<lsp_types::Location>, ClientError> {
        let params = lsp_types::request::GotoTypeDefinitionParams {
            text_document_position_params: self.at(path, position)?,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let answer = self
            .socket
            .type_definition(params)
            .await
            .map_err(|error| ClientError::Protocol(error.to_string()))?;
        Ok(locations(answer))
    }

    /// Everything that implements what is at `position`.
    ///
    /// # Errors
    ///
    /// As [`Client::hover`].
    pub async fn implementation(
        &mut self,
        path: &Path,
        position: Position,
    ) -> Result<Vec<lsp_types::Location>, ClientError> {
        let params = lsp_types::request::GotoImplementationParams {
            text_document_position_params: self.at(path, position)?,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let answer = self
            .socket
            .implementation(params)
            .await
            .map_err(|error| ClientError::Protocol(error.to_string()))?;
        Ok(locations(answer))
    }

    /// Every use of what is at `position`.
    ///
    /// # Errors
    ///
    /// As [`Client::hover`].
    pub async fn references(
        &mut self,
        path: &Path,
        position: Position,
    ) -> Result<Vec<lsp_types::Location>, ClientError> {
        let params = ReferenceParams {
            text_document_position: self.at(path, position)?,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        };
        Ok(self
            .socket
            .references(params)
            .await
            .map_err(|error| ClientError::Protocol(error.to_string()))?
            .unwrap_or_default())
    }

    /// What could be typed at `position`.
    ///
    /// # Errors
    ///
    /// As [`Client::hover`].
    pub async fn completion(
        &mut self,
        path: &Path,
        position: Position,
    ) -> Result<Vec<lsp_types::CompletionItem>, ClientError> {
        let params = CompletionParams {
            text_document_position: self.at(path, position)?,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        };
        let answer = self
            .socket
            .completion(params)
            .await
            .map_err(|error| ClientError::Protocol(error.to_string()))?;
        Ok(match answer {
            Some(lsp_types::CompletionResponse::Array(items)) => items,
            Some(lsp_types::CompletionResponse::List(list)) => list.items,
            None => Vec::new(),
        })
    }

    /// What the server knows about a suggestion it has already offered.
    ///
    /// Servers send a list quickly and the documentation only when asked, because a hundred
    /// suggestions with a paragraph each is a hundred paragraphs nobody will read. The one the
    /// caret is resting on is the one worth asking about.
    ///
    /// # Errors
    ///
    /// As [`Client::hover`].
    pub async fn resolve_completion(
        &mut self,
        item: lsp_types::CompletionItem,
    ) -> Result<lsp_types::CompletionItem, ClientError> {
        // A server that does not resolve gives the item back unchanged, so the caller has one
        // shape of answer to deal with.
        if self
            .capabilities
            .completion_provider
            .as_ref()
            .and_then(|provider| provider.resolve_provider)
            != Some(true)
        {
            return Ok(item);
        }
        self.socket
            .completion_item_resolve(item)
            .await
            .map_err(|error| ClientError::Protocol(error.to_string()))
    }

    /// What the thing being called at `position` takes.
    ///
    /// # Errors
    ///
    /// As [`Client::hover`].
    pub async fn signature_help(
        &mut self,
        path: &Path,
        position: Position,
    ) -> Result<Option<lsp_types::SignatureHelp>, ClientError> {
        let params = lsp_types::SignatureHelpParams {
            text_document_position_params: self.at(path, position)?,
            work_done_progress_params: WorkDoneProgressParams::default(),
            context: None,
        };
        self.socket
            .signature_help(params)
            .await
            .map_err(|error| ClientError::Protocol(error.to_string()))
    }

    /// Whether what is at `position` can be renamed, and what exactly would be.
    ///
    /// Asked before the rename, so the box opens over the symbol. It also lets a key pressed on
    /// a keyword say "no" before somebody types a new name for it.
    ///
    /// `Ok(None)` comes from a server that does not offer this. It is no refusal, and the caller
    /// falls back to the word under the caret.
    ///
    /// # Errors
    ///
    /// As [`Client::hover`].
    pub async fn prepare_rename(
        &mut self,
        path: &Path,
        position: Position,
    ) -> Result<Option<lsp_types::PrepareRenameResponse>, ClientError> {
        let prepares = matches!(
            self.capabilities.rename_provider,
            Some(lsp_types::OneOf::Right(lsp_types::RenameOptions {
                prepare_provider: Some(true),
                ..
            }))
        );
        if !prepares {
            return Ok(None);
        }
        self.socket
            .prepare_rename(self.at(path, position)?)
            .await
            .map_err(|error| ClientError::Protocol(error.to_string()))
    }

    /// Renames what is at `position` to `to`, everywhere.
    ///
    /// # Errors
    ///
    /// As [`Client::hover`].
    pub async fn rename(
        &mut self,
        path: &Path,
        position: Position,
        to: &str,
    ) -> Result<Option<lsp_types::WorkspaceEdit>, ClientError> {
        let params = lsp_types::RenameParams {
            text_document_position: self.at(path, position)?,
            new_name: to.to_owned(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        self.socket
            .rename(params)
            .await
            .map_err(|error| ClientError::Protocol(error.to_string()))
    }

    /// What the server could do about `range`.
    ///
    /// The diagnostics overlapping the range are sent with it, because that is how a server knows
    /// which fixes to offer: without them `rust-analyzer` offers refactors and no quick fixes.
    ///
    /// # Errors
    ///
    /// As [`Client::hover`].
    pub async fn code_action(
        &mut self,
        path: &Path,
        range: lsp_types::Range,
        diagnostics: Vec<lsp_types::Diagnostic>,
    ) -> Result<Vec<lsp_types::CodeActionOrCommand>, ClientError> {
        let Some(uri) = crate::convert::uri_of(path) else {
            return Ok(Vec::new());
        };
        let params = lsp_types::CodeActionParams {
            text_document: TextDocumentIdentifier { uri },
            range,
            context: lsp_types::CodeActionContext {
                diagnostics,
                only: None,
                trigger_kind: Some(lsp_types::CodeActionTriggerKind::INVOKED),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        Ok(self
            .socket
            .code_action(params)
            .await
            .map_err(|error| ClientError::Protocol(error.to_string()))?
            .unwrap_or_default())
    }

    /// The edit a code action carries, asked for separately.
    ///
    /// Servers advertise actions cheaply and compute their edits only for the one that is chosen,
    /// which is why an action can arrive with a title and nothing else.
    ///
    /// # Errors
    ///
    /// As [`Client::hover`].
    pub async fn resolve_code_action(
        &mut self,
        action: lsp_types::CodeAction,
    ) -> Result<lsp_types::CodeAction, ClientError> {
        if action.edit.is_some() {
            return Ok(action);
        }
        self.socket
            .code_action_resolve(action.clone())
            .await
            .map_err(|error| ClientError::Protocol(error.to_string()))
            // A server that says it resolves and then refuses is a server whose action is still
            // worth trying as it arrived, which is usually a bare command.
            .or(Ok(action))
    }

    /// Runs a command the server offered.
    ///
    /// # Errors
    ///
    /// As [`Client::hover`].
    pub async fn execute_command(
        &mut self,
        command: lsp_types::Command,
    ) -> Result<(), ClientError> {
        let params = lsp_types::ExecuteCommandParams {
            command: command.command,
            arguments: command.arguments.unwrap_or_default(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        self.socket
            .execute_command(params)
            .await
            .map(|_| ())
            .map_err(|error| ClientError::Protocol(error.to_string()))
    }

    /// Everything the file declares, in the order and nesting the server sees it.
    ///
    /// # Errors
    ///
    /// As [`Client::hover`].
    pub async fn document_symbols(
        &mut self,
        path: &Path,
    ) -> Result<Option<lsp_types::DocumentSymbolResponse>, ClientError> {
        let Some(uri) = crate::convert::uri_of(path) else {
            return Ok(None);
        };
        let params = lsp_types::DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        self.socket
            .document_symbol(params)
            .await
            .map_err(|error| ClientError::Protocol(error.to_string()))
    }

    /// Everything in the project whose name matches `query`.
    ///
    /// # Errors
    ///
    /// As [`Client::hover`].
    pub async fn workspace_symbols(&mut self, query: &str) -> Result<Vec<Symbol>, ClientError> {
        let params = lsp_types::WorkspaceSymbolParams {
            query: query.to_owned(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let answer = self
            .socket
            .symbol(params)
            .await
            .map_err(|error| ClientError::Protocol(error.to_string()))?;
        Ok(symbols(answer))
    }

    /// The other places in this file the symbol at `position` is used.
    ///
    /// # Errors
    ///
    /// As [`Client::hover`].
    pub async fn document_highlight(
        &mut self,
        path: &Path,
        position: Position,
    ) -> Result<Vec<lsp_types::DocumentHighlight>, ClientError> {
        if self.capabilities.document_highlight_provider.is_none() {
            return Ok(Vec::new());
        }
        let params = lsp_types::DocumentHighlightParams {
            text_document_position_params: self.at(path, position)?,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        Ok(self
            .socket
            .document_highlight(params)
            .await
            .map_err(|error| ClientError::Protocol(error.to_string()))?
            .unwrap_or_default())
    }

    /// How the server would lay `range` out.
    ///
    /// # Errors
    ///
    /// As [`Client::hover`].
    pub async fn format_range(
        &mut self,
        path: &Path,
        range: lsp_types::Range,
        tab_size: u32,
        spaces: bool,
    ) -> Result<Vec<TextEdit>, ClientError> {
        let Some(uri) = crate::convert::uri_of(path) else {
            return Ok(Vec::new());
        };
        let params = lsp_types::DocumentRangeFormattingParams {
            text_document: TextDocumentIdentifier { uri },
            range,
            options: lsp_types::FormattingOptions {
                tab_size,
                insert_spaces: spaces,
                ..lsp_types::FormattingOptions::default()
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        Ok(self
            .socket
            .range_formatting(params)
            .await
            .map_err(|error| ClientError::Protocol(error.to_string()))?
            .unwrap_or_default())
    }

    /// How the server would lay the file out.
    ///
    /// # Errors
    ///
    /// As [`Client::hover`].
    pub async fn format(
        &mut self,
        path: &Path,
        tab_size: u32,
        spaces: bool,
    ) -> Result<Vec<TextEdit>, ClientError> {
        let Some(uri) = crate::convert::uri_of(path) else {
            return Ok(Vec::new());
        };
        let params = DocumentFormattingParams {
            text_document: TextDocumentIdentifier { uri },
            options: lsp_types::FormattingOptions {
                tab_size,
                insert_spaces: spaces,
                ..lsp_types::FormattingOptions::default()
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        Ok(self
            .socket
            .formatting(params)
            .await
            .map_err(|error| ClientError::Protocol(error.to_string()))?
            .unwrap_or_default())
    }

    /// Asks it to stop, and waits for it to say it has.
    ///
    /// # Errors
    ///
    /// As [`Client::hover`]. A server that will not shut down politely is killed when the process
    /// handle is dropped, so an error here is worth logging and nothing more.
    pub async fn shutdown(&mut self) -> Result<(), ClientError> {
        self.socket
            .shutdown(())
            .await
            .map_err(|error| ClientError::Protocol(error.to_string()))?;
        let _ = self.socket.exit(());
        Ok(())
    }

    /// A position in a file, as a request wants it.
    fn at(
        &self,
        path: &Path,
        position: Position,
    ) -> Result<TextDocumentPositionParams, ClientError> {
        let uri = crate::convert::uri_of(path).ok_or_else(|| {
            ClientError::Protocol(format!(
                "{} is not a file a server can name",
                path.display()
            ))
        })?;
        Ok(TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        })
    }
}
