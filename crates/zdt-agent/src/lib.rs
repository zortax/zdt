//! The agent vocabulary.
//!
//! What a thread is, what an adapter reports, and what crosses the daemon's socket. Pure data:
//! no process, no database, no view. The daemon and the editor both speak in these types, which
//! is what keeps them one protocol rather than two dialects.
//!
//! # Compatibility
//!
//! The daemon and the editor are installed together but restart apart. Every struct on the wire
//! takes unknown fields quietly and fills missing ones with defaults, and every closed enum has
//! an `Unknown` catch-all, so one side a release ahead never breaks the other.

pub mod ask;
pub mod catalog;
pub mod change;
pub mod event;
pub mod mode;
pub mod protocol;
pub mod runner;
pub mod thread;
pub mod todo;
pub mod wire;

/// Which conversation the socket speaks. Bumped only when a message changes shape incompatibly.
pub const VERSION: u32 = 10;

/// What went wrong on the wire.
#[derive(Debug, thiserror::Error)]
pub enum WireError {
    /// The stream broke.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// The bytes are not a message.
    #[error("the message did not read: {0}")]
    Malformed(String),
    /// A frame larger than anything this protocol sends.
    #[error("a frame of {0} bytes is too large")]
    TooLarge(u32),
}
