//! Finding the zdt that is already running, or becoming it.
//!
//! Blocking, and with no async runtime: this runs before the editor decides whether it is going to
//! be an editor at all, and a client that has to start a runtime to say one sentence is a client
//! that costs more than it saves.
//!
//! # Why the socket is the liveness test
//!
//! A lock file holds a process id, and a process id is reused. A `connect` that is *accepted*
//! cannot be: something is listening on that socket right now. So the lock only serialises the
//! moment between "connect failed" and "bind", and the socket decides whether anybody is there.

use std::io::ErrorKind;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use crate::{IpcError, Request, Response, VERSION, frame};

/// How long to keep trying for the lock before giving up and running alone.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(2);

/// What this process should do.
pub enum HandOff {
    /// A running zdt took it. This process is done.
    Delivered(Response),
    /// Nothing was running. This process is the host, and this is what it listens on.
    Host(UnixListener),
    /// There is nowhere to put a socket, or somebody else's is in the way. Run as one editor
    /// with no way to be talked to, which is worse than not starting at all by a wide margin.
    Alone,
}

/// Where the socket and the lock live.
///
/// The runtime directory, which the desktop empties on logout — exactly right for something whose
/// meaning is "a process is running". Falling back to a directory of this user's own under `/tmp`,
/// made private and refused if it is not.
///
/// Never under the configuration directory: that one is watched recursively, and a socket there
/// would be a "configuration reloaded" toast on every connection.
#[must_use]
pub fn directory() -> Option<PathBuf> {
    if let Some(named) = std::env::var_os("ZDT_RUNTIME_DIR") {
        return Some(PathBuf::from(named));
    }
    if let Some(runtime) = dirs_runtime() {
        return Some(runtime.join("zdt"));
    }
    let fallback = std::env::temp_dir().join(format!("zdt-{}", uid()));
    Some(fallback)
}

/// The desktop's runtime directory, when it named one.
fn dirs_runtime() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)
}

/// This user's numeric id.
fn uid() -> u32 {
    // SAFETY: `getuid` cannot fail and touches nothing.
    unsafe { libc_getuid() }
}

unsafe extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
}

/// The socket inside `directory`.
#[must_use]
pub fn socket_in(directory: &Path) -> PathBuf {
    directory.join("control.sock")
}

/// The lock inside `directory`.
#[must_use]
pub fn lock_in(directory: &Path) -> PathBuf {
    directory.join("host.lock")
}

/// Hands `request` to a running zdt, or answers that this process is the one that should run.
///
/// Every failure that is not the host refusing answers [`HandOff::Alone`]: a broken runtime
/// directory must never be the reason somebody's editor will not open.
pub fn hand_off(request: &Request) -> HandOff {
    let Some(directory) = directory() else {
        return HandOff::Alone;
    };
    if make_private(&directory).is_err() {
        return HandOff::Alone;
    }

    // The lock only serialises the connect-or-bind race below. Two `zdt`s started at once must
    // not both decide nothing is listening and both bind.
    let Ok(guard) = Lock::take(&lock_in(&directory)) else {
        return HandOff::Alone;
    };

    let socket = socket_in(&directory);
    match UnixStream::connect(&socket) {
        Ok(stream) => {
            drop(guard);
            match talk(stream, request) {
                Ok(response) => HandOff::Delivered(response),
                Err(error) => {
                    // An older host is still serving real windows. It must not be evicted, so
                    // this process runs alone rather than taking the socket away.
                    tracing::warn!("the running zdt would not take it: {error}");
                    HandOff::Alone
                }
            }
        }
        // Nobody is listening. Either there never was, or the host crashed and left the socket.
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::ConnectionRefused | ErrorKind::NotFound
            ) =>
        {
            let _ = std::fs::remove_file(&socket);
            match bind(&socket) {
                Ok(listener) => {
                    let _ = std::fs::write(lock_in(&directory), std::process::id().to_string());
                    drop(guard);
                    HandOff::Host(listener)
                }
                Err(error) => {
                    tracing::warn!("cannot listen on {}: {error}", socket.display());
                    HandOff::Alone
                }
            }
        }
        Err(error) => {
            tracing::warn!("cannot reach {}: {error}", socket.display());
            HandOff::Alone
        }
    }
}

/// Says hello, asks, and reads the answer.
fn talk(mut stream: UnixStream, request: &Request) -> Result<Response, IpcError> {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
    frame::write(
        &mut stream,
        &Request::Hello {
            version: VERSION,
            pid: std::process::id(),
        },
    )?;

    match frame::read::<Response>(&mut stream)? {
        Response::Welcome { version, .. } if version == VERSION => {}
        Response::Welcome { version, .. } => {
            return Err(IpcError::Mismatched {
                theirs: version,
                ours: VERSION,
            });
        }
        Response::Refused { reason } => return Err(IpcError::Refused(reason)),
        other => {
            return Err(IpcError::Malformed(format!(
                "expected a welcome, got {other:?}"
            )));
        }
    }

    frame::write(&mut stream, request)?;
    match frame::read::<Response>(&mut stream)? {
        Response::Refused { reason } => Err(IpcError::Refused(reason)),
        answer => Ok(answer),
    }
}

/// Binds the socket, and makes it this user's alone.
fn bind(socket: &Path) -> std::io::Result<UnixListener> {
    let listener = UnixListener::bind(socket)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(listener)
}

/// Makes `directory`, and refuses it if it is not this user's own.
fn make_private(directory: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let data = std::fs::metadata(directory)?;
        if data.uid() != uid() {
            return Err(std::io::Error::new(
                ErrorKind::PermissionDenied,
                "the runtime directory belongs to somebody else",
            ));
        }
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// An exclusive hold on the lock file, released when it is dropped.
struct Lock(std::fs::File);

impl Lock {
    /// Takes it, waiting up to [`PATIENCE`].
    fn take(path: &Path) -> std::io::Result<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)?;

        let until = std::time::Instant::now() + PATIENCE;
        loop {
            if flock(&file, true).is_ok() {
                return Ok(Self(file));
            }
            if std::time::Instant::now() >= until {
                return Err(std::io::Error::new(
                    ErrorKind::WouldBlock,
                    "another zdt is starting",
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = flock(&self.0, false);
    }
}

/// Takes or gives back an exclusive advisory lock, without blocking.
fn flock(file: &std::fs::File, take: bool) -> std::io::Result<()> {
    use std::os::unix::io::AsRawFd;
    /// `LOCK_EX | LOCK_NB`.
    const TAKE: i32 = 2 | 4;
    /// `LOCK_UN`.
    const GIVE: i32 = 8;

    // SAFETY: a valid descriptor and one of the operations the call defines.
    let answer = unsafe { libc_flock(file.as_raw_fd(), if take { TAKE } else { GIVE }) };
    if answer == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

unsafe extern "C" {
    #[link_name = "flock"]
    fn libc_flock(fd: i32, operation: i32) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "zdt-ipc-{}-{name}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("the directory is made");
        directory
    }

    #[test]
    fn the_directory_can_be_told_where_to_be() {
        let before = std::env::var_os("ZDT_RUNTIME_DIR");
        unsafe { std::env::set_var("ZDT_RUNTIME_DIR", "/tmp/zdt-runtime-test") };
        assert_eq!(directory(), Some(PathBuf::from("/tmp/zdt-runtime-test")));
        match before {
            Some(held) => unsafe { std::env::set_var("ZDT_RUNTIME_DIR", held) },
            None => unsafe { std::env::remove_var("ZDT_RUNTIME_DIR") },
        }
    }

    #[test]
    fn a_lock_is_held_by_one_at_a_time() {
        let directory = temporary("lock");
        let path = lock_in(&directory);
        let held = Lock::take(&path).expect("the first takes it");
        assert!(Lock::take(&path).is_err(), "the second waits and gives up");
        drop(held);
        assert!(
            Lock::take(&path).is_ok(),
            "and gets it once the first lets go"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_socket_nobody_is_listening_on_is_taken_over() {
        // What a host that crashed leaves behind. The file is there, and connecting to it is
        // refused, which is how the next zdt knows it may take it.
        let directory = temporary("stale");
        let socket = socket_in(&directory);
        std::fs::write(&socket, b"not a socket").expect("it writes");
        assert!(socket.exists());

        let error = UnixStream::connect(&socket).expect_err("nothing is listening");
        assert!(matches!(
            error.kind(),
            ErrorKind::ConnectionRefused | ErrorKind::NotFound | ErrorKind::Other
        ));
        let _ = std::fs::remove_file(&socket);
        assert!(bind(&socket).is_ok(), "and it can be bound again");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_bound_socket_is_this_users_alone() {
        use std::os::unix::fs::PermissionsExt;
        let directory = temporary("modes");
        make_private(&directory).expect("it is made");
        let socket = socket_in(&directory);
        let _listener = bind(&socket).expect("it binds");

        let mode = std::fs::metadata(&socket)
            .expect("it reads")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let mode = std::fs::metadata(&directory)
            .expect("it reads")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_client_and_a_host_can_hold_a_conversation() {
        let directory = temporary("talk");
        let socket = socket_in(&directory);
        let listener = bind(&socket).expect("it binds");

        // A host, answering one client the way `serve` does.
        let host = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("a client arrives");
            let hello: Request = frame::read(&mut stream).expect("it reads");
            assert!(matches!(hello, Request::Hello { .. }));
            frame::write(
                &mut stream,
                &Response::Welcome {
                    version: VERSION,
                    host_pid: 7,
                },
            )
            .expect("it writes");

            let asked: Request = frame::read(&mut stream).expect("it reads");
            let Request::Attach { dir, .. } = asked else {
                panic!("an attach");
            };
            frame::write(
                &mut stream,
                &Response::Attached {
                    dir,
                    created: true,
                    focused: false,
                },
            )
            .expect("it writes");
        });

        let stream = UnixStream::connect(&socket).expect("it connects");
        let answer = talk(
            stream,
            &Request::Attach {
                dir: PathBuf::from("/home/someone/work"),
                files: Vec::new(),
                new_window: false,
            },
        )
        .expect("the host answered");

        let Response::Attached { dir, created, .. } = answer else {
            panic!("an attachment");
        };
        assert_eq!(dir, PathBuf::from("/home/someone/work"));
        assert!(created);
        host.join().expect("the host finished");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_host_that_speaks_another_version_is_left_alone() {
        // It is still serving real windows. Taking its socket away would close them.
        let directory = temporary("version");
        let socket = socket_in(&directory);
        let listener = bind(&socket).expect("it binds");

        let host = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("a client arrives");
            let _: Request = frame::read(&mut stream).expect("it reads");
            frame::write(
                &mut stream,
                &Response::Welcome {
                    version: VERSION + 99,
                    host_pid: 7,
                },
            )
            .expect("it writes");
        });

        let stream = UnixStream::connect(&socket).expect("it connects");
        let error = talk(stream, &Request::Ping).expect_err("it refuses");
        assert!(matches!(error, IpcError::Mismatched { .. }));
        host.join().expect("the host finished");
        assert!(socket.exists(), "the socket was left where it was");
        let _ = std::fs::remove_dir_all(&directory);
    }
}
