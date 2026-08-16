use std::ops::ControlFlow;
use std::path::Path;
use std::process::Stdio;
use std::sync::mpsc::Sender;

use async_lsp::LanguageServer;
use async_lsp::concurrency::ConcurrencyLayer;
use async_lsp::panic::CatchUnwindLayer;
use async_lsp::router::Router;
use lsp_types::notification::{LogMessage, Progress, PublishDiagnostics, ShowMessage};
use lsp_types::{InitializeParams, InitializeResult, InitializedParams, Url, WorkspaceFolder};
use tower::ServiceBuilder;

use crate::convert::Encoding;
use crate::registry::Wanted;

use super::{Client, ClientError, Notice, Reporting};
use crate::client::capabilities::capabilities;

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
                    // Logged and not shown. A server that logs every keystroke would own the
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
        // The child goes into the task and stays in scope. `kill_on_drop` means dropping it
        // kills the server, so leaving it behind here kills the process the moment this function
        // returns. That is exactly what happened the first time. The handshake succeeded and the
        // server was gone a moment later, reporting EOF.
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

        // Most servers read their configuration from this notification and never ask for it.
        // They want it immediately after the handshake.
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
