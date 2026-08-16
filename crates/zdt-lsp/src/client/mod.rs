//! One language server, and the way to talk to it.
//!
//! An `async-lsp` main loop on a worker, a socket the interface holds, and a channel back for
//! everything the server says without being asked.
//!
//! # Why a channel
//!
//! What the server sends arrives on the main loop's thread. Everything the interface reads is `Rc`
//! and belongs to the interface thread. A callback would have to be `Send`, and nothing on that
//! side is. So the router pushes onto a channel, and the interface drains it when it draws. The
//! grep results use the same arrangement, for the same reason.
//!
//! # What is not here
//!
//! Deciding when to ask. This starts a server, keeps its documents in step, and answers requests.
//! Which request a key means, and what to draw with the answer, belongs to the application.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use async_lsp::ServerSocket;
use lsp_types::Url;

use crate::convert::Encoding;

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

mod capabilities;
mod documents;
mod requests;
mod start;
mod symbol;
#[cfg(test)]
mod tests;

pub use crate::client::capabilities::wants_incremental;
pub use crate::client::symbol::Symbol;
