//! Where a request goes, and where the answer comes from.
//!
//! One trait, so that "the zdt running on this machine" and "a zdt running at the far end of an
//! ssh connection" are two implementations of the same thing rather than two code paths through
//! the editor.
//!
//! # What a remote session would add, and what it would not
//!
//! The tempting split — the whole session moves to the far end — is wrong in one way worth being
//! precise about. A session holds a rope, and mirroring text edits across a wire is a conflict
//! resolution problem nobody asked for. The far end does not need the rope. What has to be at the
//! far end is everything that touches *that machine's* filesystem and processes:
//!
//! | stays here | goes there |
//! |---|---|
//! | the window, the modal layer, the pickers | the language servers |
//! | the buffers, the splits, the undo history | the repository |
//! | the theme and the keymap | the terminals |
//! | | reading, writing, walking and grepping files |
//!
//! So the work is not in this trait. It is in making sure nothing outside a small number of seams
//! ever calls `std::fs` or `std::process` directly. This trait is only how a client finds a host
//! and asks it for a session; [`Local`] is the one implementation there is today.

use crate::{IpcError, Request, Response};

/// Somewhere requests can be sent.
///
/// Blocking, because both of today's callers are: the launch-time client, which runs before any
/// runtime exists, and the tests.
pub trait Transport {
    /// Asks, and waits for the answer.
    ///
    /// # Errors
    ///
    /// When the far end cannot be reached, or refuses.
    fn request(&mut self, request: &Request) -> Result<Response, IpcError>;

    /// What to call this connection, for a message about it.
    fn describe(&self) -> String;
}

/// A zdt on this machine, over a unix socket.
pub struct Local {
    stream: std::os::unix::net::UnixStream,
}

impl Local {
    /// Wraps a stream that has already been greeted.
    #[must_use]
    pub fn new(stream: std::os::unix::net::UnixStream) -> Self {
        Self { stream }
    }
}

impl Transport for Local {
    fn request(&mut self, request: &Request) -> Result<Response, IpcError> {
        crate::frame::write(&mut self.stream, request)?;
        match crate::frame::read::<Response>(&mut self.stream)? {
            Response::Refused { reason } => Err(IpcError::Refused(reason)),
            answer => Ok(answer),
        }
    }

    fn describe(&self) -> String {
        "the zdt on this machine".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A transport that answers from a list, for testing whatever holds one.
    struct Rehearsed(Vec<Response>);

    impl Transport for Rehearsed {
        fn request(&mut self, _: &Request) -> Result<Response, IpcError> {
            self.0
                .pop()
                .ok_or_else(|| IpcError::Malformed("nothing left to say".to_owned()))
        }

        fn describe(&self) -> String {
            "a rehearsal".to_owned()
        }
    }

    #[test]
    fn anything_that_answers_requests_is_a_transport() {
        // Which is the whole point: a remote host is a second implementation and not a second
        // code path through the editor.
        let mut transport = Rehearsed(vec![Response::Attached {
            dir: PathBuf::from("/x"),
            created: false,
            focused: true,
        }]);
        let answer = transport.request(&Request::Ping).expect("it answers");
        assert!(matches!(answer, Response::Attached { focused: true, .. }));
    }

    #[test]
    fn a_transport_with_nothing_left_to_say_is_an_error() {
        let mut transport = Rehearsed(Vec::new());
        assert!(transport.request(&Request::Ping).is_err());
    }
}
