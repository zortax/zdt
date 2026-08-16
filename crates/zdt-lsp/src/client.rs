//! One language server, and the way to talk to it.
//!
//! An `async-lsp` main loop on a worker, a socket the interface holds, and a channel back for
//! everything the server says without being asked.
//!
//! # Why a channel rather than callbacks
//!
//! What the server sends arrives on the main loop's thread, and everything the interface reads is
//! `Rc` and belongs to the interface thread. A callback would have to be `Send`, which nothing on
//! that side is. So the router pushes onto a channel and the interface drains it when it draws —
//! the same arrangement the grep results use, for the same reason.
//!
//! # What is not here
//!
//! Deciding when to ask. This starts a server, keeps its documents in step, and answers requests;
//! *which* request a key means, and what to draw with the answer, belongs to the application.

use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc::Sender;

use async_lsp::concurrency::ConcurrencyLayer;
use async_lsp::panic::CatchUnwindLayer;
use async_lsp::router::Router;
use async_lsp::{LanguageServer, ServerSocket};
use lsp_types::notification::{LogMessage, Progress, PublishDiagnostics, ShowMessage};
use lsp_types::{
    ClientCapabilities, CompletionClientCapabilities, CompletionItemCapability, CompletionParams,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, DocumentFormattingParams, GotoDefinitionParams,
    GotoDefinitionResponse, HoverParams, InitializeParams, InitializeResult, InitializedParams,
    InsertTextMode, MarkupKind, PartialResultParams, Position, ReferenceContext, ReferenceParams,
    TextDocumentClientCapabilities, TextDocumentContentChangeEvent, TextDocumentIdentifier,
    TextDocumentItem, TextDocumentPositionParams, TextEdit, Url, VersionedTextDocumentIdentifier,
    WorkDoneProgressParams, WorkspaceFolder,
};
use tower::ServiceBuilder;

use crate::convert::Encoding;
use crate::registry::Wanted;

/// Something a server said without being asked.
#[derive(Clone, Debug)]
pub enum Notice {
    /// Diagnostics for one file.
    Diagnostics {
        /// Which file.
        uri: Url,
        /// What is wrong with it.
        diagnostics: Vec<lsp_types::Diagnostic>,
        /// Which version of the file they are about, when the server said.
        version: Option<i32>,
    },
    /// Something to show the user.
    Message {
        /// Which server said it.
        server: String,
        /// How bad it is.
        severity: lsp_types::MessageType,
        /// What it said.
        text: String,
    },
    /// A long-running job started, moved or finished.
    Progress {
        /// Which server.
        server: String,
        /// What it is doing, when it said.
        title: Option<String>,
        /// Whether it has finished.
        done: bool,
    },
    /// The server went away.
    Exited {
        /// Which one.
        server: String,
    },
}

/// What a client failed at.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// The program could not be started.
    #[error("cannot start {command}: {source}")]
    Spawn {
        /// What was being started.
        command: String,
        /// Why it did not.
        #[source]
        source: std::io::Error,
    },
    /// The server refused, or the connection broke.
    #[error("{0}")]
    Protocol(String),
}

/// A running language server.
///
/// Cheap to clone; every clone talks to the same server.
#[derive(Clone)]
pub struct Client {
    /// The name the configuration knows it as.
    pub name: String,
    /// Where it is rooted.
    pub root: PathBuf,
    /// How it counts characters.
    pub encoding: Encoding,
    /// What it said it can do.
    pub capabilities: lsp_types::ServerCapabilities,
    socket: ServerSocket,
}

/// What the router carries: somewhere to put what the server says.
struct Reporting {
    server: String,
    notices: Sender<Notice>,
}

impl Client {
    /// Starts `wanted`, initialises it, and answers a client for it.
    ///
    /// Everything the server says without being asked goes to `notices`. Async, and meant to be
    /// awaited on the background executor: it spawns a process and waits for a round trip.
    ///
    /// # Errors
    ///
    /// If the program will not start, or the handshake fails.
    pub async fn start(wanted: &Wanted, notices: Sender<Notice>) -> Result<Self, ClientError> {
        let reporting = Reporting {
            server: wanted.name.clone(),
            notices: notices.clone(),
        };

        let (mainloop, mut socket) = async_lsp::MainLoop::new_client(|_socket| {
            let mut router = Router::new(reporting);
            router
                .notification::<PublishDiagnostics>(|this, params| {
                    let _ = this.notices.send(Notice::Diagnostics {
                        uri: params.uri,
                        diagnostics: params.diagnostics,
                        version: params.version,
                    });
                    ControlFlow::Continue(())
                })
                .notification::<ShowMessage>(|this, params| {
                    let _ = this.notices.send(Notice::Message {
                        server: this.server.clone(),
                        severity: params.typ,
                        text: params.message,
                    });
                    ControlFlow::Continue(())
                })
                .notification::<LogMessage>(|this, params| {
                    // Logged rather than shown: a server that logs every keystroke would own the
                    // status line.
                    tracing::debug!("{}: {}", this.server, params.message);
                    ControlFlow::Continue(())
                })
                .notification::<Progress>(|this, params| {
                    let (title, done) = match &params.value {
                        lsp_types::ProgressParamsValue::WorkDone(work) => match work {
                            lsp_types::WorkDoneProgress::Begin(begin) => {
                                (Some(begin.title.clone()), false)
                            }
                            lsp_types::WorkDoneProgress::Report(report) => {
                                (report.message.clone(), false)
                            }
                            lsp_types::WorkDoneProgress::End(_) => (None, true),
                        },
                    };
                    let _ = this.notices.send(Notice::Progress {
                        server: this.server.clone(),
                        title,
                        done,
                    });
                    ControlFlow::Continue(())
                })
                // Everything else a server may send is answered by not falling over. A router
                // with no arm for a notification returns an error to the server, which some take
                // as a reason to stop.
                .unhandled_notification(|_, _| ControlFlow::Continue(()));

            ServiceBuilder::new()
                .layer(CatchUnwindLayer::default())
                .layer(ConcurrencyLayer::default())
                .service(router)
        });

        let mut command = tokio::process::Command::new(&wanted.command);
        command
            .args(&wanted.args)
            .current_dir(&wanted.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        for (name, value) in &wanted.env {
            command.env(name, value);
        }

        let mut child = command.spawn().map_err(|source| ClientError::Spawn {
            command: wanted.command.clone(),
            source,
        })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ClientError::Protocol("the server has no output".to_owned()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ClientError::Protocol("the server takes no input".to_owned()))?;

        // `async-lsp` speaks `futures-io` and tokio's pipes speak tokio's own traits. The
        // compatibility wrapper is the whole of the difference.
        use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
        let (stdout, stdin) = (stdout.compat(), stdin.compat_write());

        let name = wanted.name.clone();
        let ending = notices;
        // The child goes into the task, not out of scope. `kill_on_drop` means dropping it kills
        // the server, so leaving it behind here would kill the process the moment this function
        // returned — which is exactly what happened the first time: the handshake succeeded and
        // the server was gone a moment later, reporting EOF.
        tokio::spawn(async move {
            let mut child = child;
            if let Err(error) = mainloop.run_buffered(stdout, stdin).await {
                tracing::warn!("{name}: {error}");
            }
            // The loop has ended, so the server has closed its side. Dropping the child now is
            // what stops one that will not exit on its own from staying behind.
            let _ = child.start_kill();
            let _ = child.wait().await;
            let _ = ending.send(Notice::Exited { server: name });
        });

        let root = uri_for(&wanted.root)?;
        let answer: InitializeResult = socket
            .initialize(InitializeParams {
                workspace_folders: Some(vec![WorkspaceFolder {
                    uri: root.clone(),
                    name: wanted
                        .root
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "root".to_owned()),
                }]),
                capabilities: capabilities(),
                initialization_options: wanted.initialization_options.clone(),
                ..InitializeParams::default()
            })
            .await
            .map_err(|error| ClientError::Protocol(error.to_string()))?;

        socket
            .initialized(InitializedParams {})
            .map_err(|error| ClientError::Protocol(error.to_string()))?;

        // Servers that read their configuration on notification rather than on request — most of
        // them — want this immediately after the handshake.
        if let Some(settings) = wanted.settings.clone() {
            let _ = socket
                .did_change_configuration(lsp_types::DidChangeConfigurationParams { settings });
        }

        Ok(Self {
            name: wanted.name.clone(),
            root: wanted.root.clone(),
            encoding: Encoding::of(answer.capabilities.position_encoding.as_ref()),
            capabilities: answer.capabilities,
            socket,
        })
    }

    // ---- Keeping the documents in step -------------------------------------------------------

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
    /// `changes` are incremental when the server asked for incremental sync and whole-text when it
    /// did not — which the caller decides, because only it knows what the editor reported.
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

    // ---- Asking it things ---------------------------------------------------------------------

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
        // A server that does not resolve gives the item back unchanged rather than failing, so
        // that the caller has one shape of answer to deal with.
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
    /// Asked before the rename so that the box opens over the symbol rather than over whatever
    /// happened to be under the caret — and so that a key pressed on a keyword says "no" before
    /// somebody has typed a new name for it.
    ///
    /// `Ok(None)` from a server that does not offer this at all, which is not a refusal: the
    /// caller falls back to the word under the caret.
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

/// One thing a project declares, whichever shape the server said it in.
///
/// The protocol has two answers for "what is in this project" — a flat list with locations, and a
/// nested one whose locations may be a file with no range in it — and a picker wants one. So both
/// are flattened here rather than in the four places that would otherwise have to know.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    /// What it is called.
    pub name: String,
    /// What kind of thing it is.
    pub kind: lsp_types::SymbolKind,
    /// What it is inside, when the server says.
    pub container: Option<String>,
    /// Which file it is in.
    pub uri: Url,
    /// Where in the file. The start of it when the server gave only a file.
    pub range: lsp_types::Range,
}

/// The two shapes a workspace-symbol answer comes in, as one list.
fn symbols(answer: Option<lsp_types::WorkspaceSymbolResponse>) -> Vec<Symbol> {
    use lsp_types::{OneOf, WorkspaceSymbolResponse};

    match answer {
        None => Vec::new(),
        Some(WorkspaceSymbolResponse::Flat(found)) => found
            .into_iter()
            .map(|one| Symbol {
                name: one.name,
                kind: one.kind,
                container: one.container_name,
                uri: one.location.uri,
                range: one.location.range,
            })
            .collect(),
        Some(WorkspaceSymbolResponse::Nested(found)) => found
            .into_iter()
            .map(|one| {
                let (uri, range) = match one.location {
                    OneOf::Left(location) => (location.uri, location.range),
                    // A server allowed to answer with a file and no range, which it may do when
                    // the client says it will resolve. This one does not, so the top of the file
                    // is where the symbol is taken to be — which is wrong by a screenful and
                    // right by a file, and a file is what somebody picking a symbol wanted.
                    OneOf::Right(partial) => (partial.uri, lsp_types::Range::default()),
                };
                Symbol {
                    name: one.name,
                    kind: one.kind,
                    container: one.container_name,
                    uri,
                    range,
                }
            })
            .collect(),
    }
}

/// The three shapes a "go to definition" answer comes in, as one list.
fn locations(answer: Option<GotoDefinitionResponse>) -> Vec<lsp_types::Location> {
    match answer {
        Some(GotoDefinitionResponse::Scalar(one)) => vec![one],
        Some(GotoDefinitionResponse::Array(many)) => many,
        Some(GotoDefinitionResponse::Link(links)) => links
            .into_iter()
            .map(|link| lsp_types::Location {
                uri: link.target_uri,
                range: link.target_selection_range,
            })
            .collect(),
        None => Vec::new(),
    }
}

/// A directory as a URI, or an error saying why not.
fn uri_for(root: &Path) -> Result<Url, ClientError> {
    crate::convert::uri_of(root).ok_or_else(|| {
        ClientError::Protocol(format!(
            "{} is not a path a server can name",
            root.display()
        ))
    })
}

/// What this editor tells a server it can do.
///
/// Deliberately modest. A capability claimed and not honoured is worse than one not claimed: the
/// server changes what it sends, and the difference shows up as something quietly not working.
fn capabilities() -> ClientCapabilities {
    ClientCapabilities {
        text_document: Some(TextDocumentClientCapabilities {
            completion: Some(CompletionClientCapabilities {
                completion_item: Some(CompletionItemCapability {
                    snippet_support: Some(false),
                    documentation_format: Some(vec![MarkupKind::Markdown, MarkupKind::PlainText]),
                    insert_text_mode_support: Some(lsp_types::InsertTextModeSupport {
                        value_set: vec![InsertTextMode::AS_IS],
                    }),
                    // What the popup asks for once the caret rests on a row, rather than for
                    // every row up front: a hundred suggestions with a paragraph each is a
                    // hundred paragraphs nobody will read.
                    resolve_support: Some(lsp_types::CompletionItemCapabilityResolveSupport {
                        properties: vec![
                            "documentation".to_owned(),
                            "detail".to_owned(),
                            "additionalTextEdits".to_owned(),
                        ],
                    }),
                    // The line beside the label. Claimed because the popup draws it; a server
                    // that sends it unasked is one whose extra field would be ignored.
                    label_details_support: Some(true),
                    ..CompletionItemCapability::default()
                }),
                ..CompletionClientCapabilities::default()
            }),
            hover: Some(lsp_types::HoverClientCapabilities {
                content_format: Some(vec![MarkupKind::Markdown, MarkupKind::PlainText]),
                ..lsp_types::HoverClientCapabilities::default()
            }),
            signature_help: Some(lsp_types::SignatureHelpClientCapabilities {
                signature_information: Some(lsp_types::SignatureInformationSettings {
                    documentation_format: Some(vec![MarkupKind::Markdown, MarkupKind::PlainText]),
                    // Which parameter is being typed, which is the whole point of the panel.
                    parameter_information: Some(lsp_types::ParameterInformationSettings {
                        label_offset_support: Some(true),
                    }),
                    active_parameter_support: Some(true),
                }),
                context_support: Some(false),
                ..lsp_types::SignatureHelpClientCapabilities::default()
            }),
            // Claimed with `prepare_support`, because the rename box asks first: a key pressed on
            // a keyword should say "no" before somebody has typed a new name for it.
            rename: Some(lsp_types::RenameClientCapabilities {
                prepare_support: Some(true),
                ..lsp_types::RenameClientCapabilities::default()
            }),
            // The literal form, because a bare `Command` cannot say what kind of action it is and
            // the picker groups by kind. Resolution is claimed because servers compute an action's
            // edit only for the one that is chosen.
            code_action: Some(lsp_types::CodeActionClientCapabilities {
                code_action_literal_support: Some(lsp_types::CodeActionLiteralSupport {
                    code_action_kind: lsp_types::CodeActionKindLiteralSupport {
                        value_set: vec![
                            String::new(),
                            "quickfix".to_owned(),
                            "refactor".to_owned(),
                            "refactor.extract".to_owned(),
                            "refactor.inline".to_owned(),
                            "refactor.rewrite".to_owned(),
                            "source".to_owned(),
                            "source.organizeImports".to_owned(),
                        ],
                    },
                }),
                resolve_support: Some(lsp_types::CodeActionCapabilityResolveSupport {
                    properties: vec!["edit".to_owned()],
                }),
                is_preferred_support: Some(true),
                data_support: Some(true),
                ..lsp_types::CodeActionClientCapabilities::default()
            }),
            document_symbol: Some(lsp_types::DocumentSymbolClientCapabilities {
                hierarchical_document_symbol_support: Some(true),
                ..lsp_types::DocumentSymbolClientCapabilities::default()
            }),
            document_highlight: Some(lsp_types::DocumentHighlightClientCapabilities::default()),
            declaration: Some(lsp_types::GotoCapability::default()),
            type_definition: Some(lsp_types::GotoCapability::default()),
            implementation: Some(lsp_types::GotoCapability::default()),
            formatting: Some(lsp_types::DocumentFormattingClientCapabilities::default()),
            range_formatting: Some(lsp_types::DocumentRangeFormattingClientCapabilities::default()),
            publish_diagnostics: Some(lsp_types::PublishDiagnosticsClientCapabilities {
                version_support: Some(true),
                ..lsp_types::PublishDiagnosticsClientCapabilities::default()
            }),
            synchronization: Some(lsp_types::TextDocumentSyncClientCapabilities {
                did_save: Some(true),
                ..lsp_types::TextDocumentSyncClientCapabilities::default()
            }),
            ..TextDocumentClientCapabilities::default()
        }),
        workspace: Some(lsp_types::WorkspaceClientCapabilities {
            // A rename crosses files, so the edit that carries it does too. `document_changes`
            // is what makes the edit versioned, which is what lets an edit against a buffer that
            // has moved on be refused rather than applied to the wrong text.
            workspace_edit: Some(lsp_types::WorkspaceEditClientCapabilities {
                document_changes: Some(true),
                resource_operations: Some(vec![
                    lsp_types::ResourceOperationKind::Create,
                    lsp_types::ResourceOperationKind::Rename,
                    lsp_types::ResourceOperationKind::Delete,
                ]),
                failure_handling: Some(lsp_types::FailureHandlingKind::Abort),
                ..lsp_types::WorkspaceEditClientCapabilities::default()
            }),
            symbol: Some(lsp_types::WorkspaceSymbolClientCapabilities::default()),
            execute_command: Some(lsp_types::DynamicRegistrationClientCapabilities::default()),
            ..lsp_types::WorkspaceClientCapabilities::default()
        }),
        window: Some(lsp_types::WindowClientCapabilities {
            work_done_progress: Some(true),
            ..lsp_types::WindowClientCapabilities::default()
        }),
        general: Some(lsp_types::GeneralClientCapabilities {
            position_encodings: Some(vec![
                lsp_types::PositionEncodingKind::UTF8,
                lsp_types::PositionEncodingKind::UTF16,
            ]),
            ..lsp_types::GeneralClientCapabilities::default()
        }),
        ..ClientCapabilities::default()
    }
}

/// Whether a server wants changes one at a time or the whole file each time.
#[must_use]
pub fn wants_incremental(capabilities: &lsp_types::ServerCapabilities) -> bool {
    use lsp_types::{TextDocumentSyncCapability, TextDocumentSyncKind};

    match &capabilities.text_document_sync {
        Some(TextDocumentSyncCapability::Kind(kind)) => *kind == TextDocumentSyncKind::INCREMENTAL,
        Some(TextDocumentSyncCapability::Options(options)) => {
            options.change == Some(TextDocumentSyncKind::INCREMENTAL)
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_shapes_of_a_definition_answer_are_one_list() {
        let uri = Url::parse("file:///a.rs").expect("a uri");
        let range = lsp_types::Range::default();
        let one = lsp_types::Location {
            uri: uri.clone(),
            range,
        };

        assert_eq!(
            locations(Some(GotoDefinitionResponse::Scalar(one.clone()))).len(),
            1
        );
        assert_eq!(
            locations(Some(GotoDefinitionResponse::Array(vec![
                one.clone(),
                one.clone()
            ])))
            .len(),
            2
        );
        assert_eq!(
            locations(Some(GotoDefinitionResponse::Link(vec![
                lsp_types::LocationLink {
                    origin_selection_range: None,
                    target_uri: uri,
                    target_range: range,
                    target_selection_range: range,
                }
            ])))
            .len(),
            1
        );
        assert!(locations(None).is_empty());
    }

    #[test]
    fn incremental_sync_is_only_claimed_when_it_is_asked_for() {
        use lsp_types::{
            ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind,
            TextDocumentSyncOptions,
        };

        let full = ServerCapabilities {
            text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
            ..ServerCapabilities::default()
        };
        assert!(!wants_incremental(&full));

        let incremental = ServerCapabilities {
            text_document_sync: Some(TextDocumentSyncCapability::Kind(
                TextDocumentSyncKind::INCREMENTAL,
            )),
            ..ServerCapabilities::default()
        };
        assert!(wants_incremental(&incremental));

        let spelled_out = ServerCapabilities {
            text_document_sync: Some(TextDocumentSyncCapability::Options(
                TextDocumentSyncOptions {
                    change: Some(TextDocumentSyncKind::INCREMENTAL),
                    ..TextDocumentSyncOptions::default()
                },
            )),
            ..ServerCapabilities::default()
        };
        assert!(wants_incremental(&spelled_out));

        // Said nothing: whole text, which every server understands.
        assert!(!wants_incremental(&ServerCapabilities::default()));
    }

    #[test]
    fn what_is_claimed_is_what_is_honoured() {
        let claimed = capabilities();
        let completion = claimed
            .text_document
            .as_ref()
            .and_then(|text| text.completion.as_ref())
            .and_then(|completion| completion.completion_item.as_ref())
            .expect("completion is claimed");

        assert_eq!(
            completion.snippet_support,
            Some(false),
            "snippets are not expanded yet, so they are not claimed"
        );
        assert!(
            completion.resolve_support.is_some(),
            "the popup asks for documentation a row at a time, so resolution is claimed"
        );
    }

    #[test]
    fn every_request_that_is_written_is_also_claimed() {
        // A capability claimed and not honoured is worse than one not claimed: the server changes
        // what it sends, and the difference shows up as something quietly not working. So is the
        // reverse — a request sent without the capability is one a strict server may refuse.
        let claimed = capabilities();
        let text = claimed
            .text_document
            .as_ref()
            .expect("text-document capabilities are claimed");

        assert!(text.declaration.is_some(), "declaration is asked for");
        assert!(
            text.type_definition.is_some(),
            "type_definition is asked for"
        );
        assert!(text.implementation.is_some(), "implementation is asked for");
        assert!(text.signature_help.is_some(), "signature_help is asked for");
        assert!(text.rename.is_some(), "rename is asked for");
        assert!(text.code_action.is_some(), "code_action is asked for");
        assert!(
            text.document_symbol.is_some(),
            "document_symbol is asked for"
        );
        assert!(
            text.document_highlight.is_some(),
            "document_highlight is asked for"
        );
        assert!(
            text.range_formatting.is_some(),
            "range_formatting is asked for"
        );

        let workspace = claimed
            .workspace
            .as_ref()
            .expect("workspace capabilities are claimed");
        assert!(
            workspace.symbol.is_some(),
            "workspace symbols are asked for"
        );
        assert!(
            workspace.execute_command.is_some(),
            "a code action's command is run"
        );
    }

    #[test]
    fn a_rename_says_it_will_ask_first() {
        // Which is what lets a key pressed on a keyword say "no" before somebody has typed a new
        // name for it, rather than after.
        let rename = capabilities()
            .text_document
            .and_then(|text| text.rename)
            .expect("rename is claimed");
        assert_eq!(rename.prepare_support, Some(true));
    }

    #[test]
    fn a_workspace_edit_is_versioned_and_may_move_files() {
        // Unversioned, an edit that arrives after the buffer has moved on is applied to the wrong
        // text with no way to tell. Without the resource operations, a rename that moves a file —
        // which renaming a Rust module does — silently loses the move.
        let edit = capabilities()
            .workspace
            .and_then(|workspace| workspace.workspace_edit)
            .expect("workspace edits are claimed");
        assert_eq!(edit.document_changes, Some(true));
        assert_eq!(
            edit.resource_operations.map(|ops| ops.len()),
            Some(3),
            "create, rename and delete"
        );
    }

    #[test]
    fn both_shapes_of_workspace_symbol_flatten_to_one() {
        use lsp_types::{
            Location, OneOf, Position, Range, SymbolInformation, SymbolKind, WorkspaceSymbol,
            WorkspaceSymbolResponse,
        };

        let uri = Url::parse("file:///project/src/lib.rs").expect("a url");
        let range = Range::new(Position::new(3, 0), Position::new(3, 8));

        #[allow(deprecated)]
        let flat = WorkspaceSymbolResponse::Flat(vec![SymbolInformation {
            name: "run".to_owned(),
            kind: SymbolKind::FUNCTION,
            tags: None,
            deprecated: None,
            location: Location {
                uri: uri.clone(),
                range,
            },
            container_name: Some("app".to_owned()),
        }]);
        let found = symbols(Some(flat));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "run");
        assert_eq!(found[0].range, range);
        assert_eq!(found[0].container.as_deref(), Some("app"));

        // The same symbol said the other way round comes out identical, which is the whole point:
        // a picker over these must not be able to tell which shape the server chose.
        let nested = WorkspaceSymbolResponse::Nested(vec![WorkspaceSymbol {
            name: "run".to_owned(),
            kind: SymbolKind::FUNCTION,
            tags: None,
            container_name: Some("app".to_owned()),
            location: OneOf::Left(Location {
                uri: uri.clone(),
                range,
            }),
            data: None,
        }]);
        assert_eq!(symbols(Some(nested)), found);
    }

    #[test]
    fn a_symbol_with_no_range_lands_at_the_top_of_its_file() {
        // Wrong by a screenful and right by a file, and a file is what somebody picking a symbol
        // out of a project of ten thousand was actually after.
        use lsp_types::{
            OneOf, SymbolKind, WorkspaceLocation, WorkspaceSymbol, WorkspaceSymbolResponse,
        };

        let uri = Url::parse("file:///project/src/lib.rs").expect("a url");
        let answer = WorkspaceSymbolResponse::Nested(vec![WorkspaceSymbol {
            name: "run".to_owned(),
            kind: SymbolKind::FUNCTION,
            tags: None,
            container_name: None,
            location: OneOf::Right(WorkspaceLocation { uri: uri.clone() }),
            data: None,
        }]);

        let found = symbols(Some(answer));
        assert_eq!(found[0].uri, uri);
        assert_eq!(found[0].range, lsp_types::Range::default());
    }

    #[test]
    fn nothing_at_all_is_an_empty_list_rather_than_an_error() {
        assert!(symbols(None).is_empty());
        assert!(locations(None).is_empty());
    }
}
