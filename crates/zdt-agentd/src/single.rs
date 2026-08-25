//! One daemon per user.
//!
//! The same shape as the editor's own single-instance claim: the socket is the liveness test,
//! and the lock only serialises the moment between "connect failed" and "bind". See
//! `zdt_ipc::client` for why a lock file alone cannot be trusted.

use std::io::ErrorKind;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

/// How long to wait for the lock before deciding another daemon is starting.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(2);

/// The socket the daemon listens on.
#[must_use]
pub fn socket_in(directory: &Path) -> PathBuf {
    directory.join("agentd.sock")
}

/// Claims the daemon's socket, or answers that a daemon already has it.
///
/// # Errors
///
/// When there is no runtime directory, or the socket cannot be bound.
pub fn claim() -> anyhow::Result<Option<UnixListener>> {
    let directory = zdt_ipc::client::directory()
        .ok_or_else(|| anyhow::anyhow!("there is no runtime directory"))?;
    make_private(&directory)?;

    let _guard = Lock::take(&directory.join("agentd.lock"))?;
    let socket = socket_in(&directory);
    match UnixStream::connect(&socket) {
        // Something is listening. That is the daemon.
        Ok(_) => Ok(None),
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::ConnectionRefused | ErrorKind::NotFound
            ) =>
        {
            let _ = std::fs::remove_file(&socket);
            let listener = UnixListener::bind(&socket)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
            }
            Ok(Some(listener))
        }
        Err(error) => Err(error.into()),
    }
}

/// Makes `directory`, and refuses it if it is not this user's own.
fn make_private(directory: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// An exclusive hold on the lock file, released when it is dropped.
struct Lock(std::fs::File);

impl Lock {
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
                    "another daemon is starting",
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
