//! Answering the clients that hand directories over.
//!
//! The other half of [`zdt_ipc::client`]. A `zdt` started on a directory this one already owns
//! does not become a second editor: it says so over the socket and exits, and this is what
//! listens.
//!
//! # Why a thread and a queue rather than an async listener
//!
//! Carrying a request out means opening a window and touching reactive state, which is the
//! interface thread's alone. And the background runtime's context is only entered around a frame,
//! so a socket cannot simply be awaited from anywhere.
//!
//! So: one thread accepts connections and reads them, a queue carries each request to the
//! interface thread, and the answer goes back the same way. The same shape the language layer
//! uses for what a server says unasked.

use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};

use zdt_ipc::{Request, Response, SessionInfo, VERSION, frame};

use crate::session::SessionKey;
use crate::session::host::{Revealed, SessionHost};

/// How often the queue is looked at. One frame: a launch is not a hot path, and a person waiting
/// on a window cannot see sixteen milliseconds.
const DRAIN: std::time::Duration = std::time::Duration::from_millis(16);

/// One request, and where its answer goes.
struct Asked {
    request: Request,
    answer: Sender<Response>,
}

impl SessionHost {
    /// Starts answering clients on `listener`.
    pub fn serve(&self, listener: std::os::unix::net::UnixListener) {
        let (asked, inbox) = channel::<Asked>();

        // One thread for the socket. Every request is two small messages each way, so it spends
        // its life blocked on `accept`.
        if std::thread::Builder::new()
            .name("zdt-control".to_owned())
            .spawn(move || accept_forever(&listener, &asked))
            .is_err()
        {
            tracing::warn!("cannot listen for other zdts; running alone");
            return;
        }

        let host = self.clone();
        let job = self.clock().every(DRAIN, move || {
            for Asked { request, answer } in inbox.try_iter() {
                let _ = answer.send(host.carry_out(request));
            }
        });
        // Held for the application's life, and stopped only when it ends.
        std::mem::forget(job);
    }

    /// What one request comes to. On the interface thread.
    fn carry_out(&self, request: Request) -> Response {
        match request {
            Request::Ping => Response::Pong,
            Request::List => Response::Sessions {
                sessions: self
                    .list_untracked()
                    .into_iter()
                    .map(|listed| SessionInfo {
                        dir: listed.key.path().map(PathBuf::from).unwrap_or_default(),
                        name: listed.name,
                        buffers: listed.buffers,
                        attached: listed.attached,
                    })
                    .collect(),
            },
            Request::Attach {
                dir,
                files,
                new_window,
            } => match SessionKey::of(&dir) {
                Some(key) => {
                    let revealed = if new_window {
                        self.reveal_in_new_window(key, &files)
                    } else {
                        self.reveal(key, &files)
                    };
                    Response::Attached {
                        dir,
                        created: revealed == Revealed::Opened,
                        focused: revealed == Revealed::Focused,
                    }
                }
                None => Response::Refused {
                    reason: format!("{} is not a directory", dir.display()),
                },
            },
            Request::Kill { dir } => match SessionKey::of(&dir).and_then(|key| self.find(&key)) {
                Some(id) if self.kill(id) => Response::Killed { dir },
                Some(_) => Response::Refused {
                    reason: "that is the only session".to_owned(),
                },
                None => Response::Refused {
                    reason: format!("{} is not open", dir.display()),
                },
            },
            Request::Hello { .. } => Response::Refused {
                reason: "already said hello".to_owned(),
            },
        }
    }
}

/// Takes clients one at a time, for as long as the socket is there.
fn accept_forever(listener: &std::os::unix::net::UnixListener, asked: &Sender<Asked>) {
    loop {
        match listener.accept() {
            Ok((stream, _)) => converse(stream, asked),
            Err(error) => {
                tracing::warn!("a client would not connect: {error}");
                return;
            }
        }
    }
}

/// One client, from hello to answer.
fn converse(mut stream: std::os::unix::net::UnixStream, asked: &Sender<Asked>) {
    // A client that says nothing must not hold the thread for ever.
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(10)));

    let greeting: Request = match frame::read(&mut stream) {
        Ok(greeting) => greeting,
        Err(error) => {
            tracing::warn!("a client said something unreadable: {error}");
            return;
        }
    };
    match greeting {
        Request::Hello { version, .. } if version == VERSION => {
            let welcome = Response::Welcome {
                version: VERSION,
                host_pid: std::process::id(),
            };
            if frame::write(&mut stream, &welcome).is_err() {
                return;
            }
        }
        Request::Hello { version, .. } => {
            // A client from another release. Refused clearly, so it can run alone rather than
            // guessing at what this one meant.
            let _ = frame::write(
                &mut stream,
                &Response::Refused {
                    reason: format!("this zdt speaks version {VERSION}, not {version}"),
                },
            );
            return;
        }
        _ => {
            let _ = frame::write(
                &mut stream,
                &Response::Refused {
                    reason: "expected a hello".to_owned(),
                },
            );
            return;
        }
    }

    let Ok(request) = frame::read::<Request>(&mut stream) else {
        return;
    };
    let (answer, waiting) = channel();
    if asked.send(Asked { request, answer }).is_err() {
        // The editor has gone. Nothing left to answer with.
        return;
    }
    // Bounded, so a client is never left waiting on an editor that has stopped drawing.
    let answer = waiting
        .recv_timeout(std::time::Duration::from_secs(30))
        .unwrap_or_else(|_| Response::Refused {
            reason: "the running zdt did not answer".to_owned(),
        });
    let _ = frame::write(&mut stream, &answer);
}
