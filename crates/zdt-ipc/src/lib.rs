//! Handing a directory to a zdt that is already running.
//!
//! One editor owns a session, because a session is `Rc`-shaped reactive state and two processes
//! cannot share one. So the second `zdt` on a directory is not a second editor: it is a *client*
//! that hands the directory over and exits, and the editor that is already running opens or
//! focuses a window for it.
//!
//! ```text
//! zdt ~/proj   -> nothing is running: this process becomes the host
//! zdt ~/other  -> hands ~/other over; the host opens a window; this process exits
//! zdt ~/proj   -> hands ~/proj over; the host focuses the window already showing it
//! ```
//!
//! # Why this crate has no zgui in it
//!
//! The client half runs before any window: it must not cost a graphics device, and it has to
//! build for a headless agent that has no display at all. That agent is also what makes this the
//! seam a remote session will use — the same messages, over ssh's stdio instead of a socket.
//!
//! # The wire
//!
//! A four-byte little-endian length, then JSON. JSON because the traffic is a handful of small
//! messages per launch and because a *stale* host, built from an older release, still has to
//! parse enough of a frame to answer [`Response::Refused`]. Both ends are generous about fields
//! they do not know, for the same reason.

pub mod client;
pub mod frame;
pub mod transport;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Which conversation this is. Bumped only when a message changes shape incompatibly.
pub const VERSION: u32 = 1;

/// What a client asks a host to do.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "snake_case")]
pub enum Request {
    /// Always first, so a version that cannot be talked to is a clean refusal.
    Hello {
        /// Which conversation the client speaks.
        version: u32,
        /// Which process is asking, for the log.
        pid: u32,
    },
    /// Put this directory on screen, and open these files in it.
    Attach {
        /// Which directory.
        dir: PathBuf,
        /// What to open in it.
        #[serde(default)]
        files: Vec<PathBuf>,
        /// Whether it should get a window of its own.
        #[serde(default)]
        new_window: bool,
    },
    /// What sessions are open.
    List,
    /// Take this directory's session away.
    Kill {
        /// Which directory.
        dir: PathBuf,
    },
    /// Whether anybody is there.
    Ping,
}

/// What a host answers.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "response", rename_all = "snake_case")]
pub enum Response {
    /// The host is there and speaks this conversation.
    Welcome {
        /// Which conversation the host speaks.
        version: u32,
        /// Which process it is.
        host_pid: u32,
    },
    /// The directory is on screen.
    Attached {
        /// Which directory.
        dir: PathBuf,
        /// Whether the session had to be made.
        created: bool,
        /// Whether it was already the one on screen.
        focused: bool,
    },
    /// What is open.
    ///
    /// A named field rather than a bare list: an internally-tagged enum cannot carry a sequence
    /// directly, because there would be nowhere to put the tag.
    Sessions {
        /// Every session, most recently opened last.
        sessions: Vec<SessionInfo>,
    },
    /// The session is gone.
    Killed {
        /// Which directory.
        dir: PathBuf,
    },
    /// The host will not do it, and why.
    Refused {
        /// What to tell the person.
        reason: String,
    },
    /// Still here.
    Pong,
}

/// One open session, as a listing sees it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionInfo {
    /// Which directory it is for.
    pub dir: PathBuf,
    /// What it is called.
    pub name: String,
    /// How many buffers are open in it.
    pub buffers: usize,
    /// Whether a window is looking at it.
    pub attached: bool,
}

/// What went wrong talking to a host.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    /// The socket or the lock could not be reached.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// The bytes on the wire are not a message.
    #[error("the message did not read: {0}")]
    Malformed(String),
    /// A frame larger than anything this protocol sends.
    #[error("a frame of {0} bytes is too large")]
    TooLarge(u32),
    /// The host speaks a conversation this client does not.
    #[error("the running zdt speaks version {theirs}; this one speaks {ours}")]
    Mismatched {
        /// What the host said.
        theirs: u32,
        /// What this client speaks.
        ours: u32,
    },
    /// The host said no.
    #[error("{0}")]
    Refused(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_round_trips() {
        let request = Request::Attach {
            dir: PathBuf::from("/home/someone/work"),
            files: vec![PathBuf::from("/home/someone/work/a.rs")],
            new_window: true,
        };
        let text = serde_json::to_string(&request).expect("it encodes");
        let back: Request = serde_json::from_str(&text).expect("it decodes");
        let Request::Attach { dir, files, .. } = back else {
            panic!("an attach");
        };
        assert_eq!(dir, PathBuf::from("/home/someone/work"));
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn a_field_a_later_release_added_does_not_stop_an_older_host_reading_it() {
        // Which is what makes a clean refusal possible rather than a dropped connection.
        let text = r#"{"request":"attach","dir":"/x","files":[],"colour":"blue"}"#;
        let back: Request = serde_json::from_str(text).expect("it decodes anyway");
        assert!(matches!(back, Request::Attach { .. }));
    }

    #[test]
    fn a_field_an_older_client_leaves_out_is_a_default() {
        let text = r#"{"request":"attach","dir":"/x"}"#;
        let back: Request = serde_json::from_str(text).expect("it decodes");
        let Request::Attach {
            files, new_window, ..
        } = back
        else {
            panic!("an attach");
        };
        assert!(files.is_empty());
        assert!(!new_window);
    }

    #[test]
    fn a_response_round_trips() {
        let response = Response::Sessions {
            sessions: vec![SessionInfo {
                dir: PathBuf::from("/x"),
                name: "x".to_owned(),
                buffers: 3,
                attached: true,
            }],
        };
        let text = serde_json::to_string(&response).expect("it encodes");
        let Response::Sessions { sessions } = serde_json::from_str(&text).expect("it decodes")
        else {
            panic!("a listing");
        };
        assert_eq!(sessions[0].buffers, 3);
    }
}
