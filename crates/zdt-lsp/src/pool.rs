//! One client per server and root.
//!
//! Servers start when the first file that wants one is opened, not at startup: a project with a
//! Rust crate and a Python script in it should not start `basedpyright` until somebody opens the
//! script, and starting every configured server on every project would make opening an editor cost
//! whatever the slowest of them costs.
//!
//! # What "starting" means here
//!
//! Asking for a client answers immediately with what is running. A server that is not yet up is
//! *begun* — the caller is told so and gets nothing this time — and the file it was wanted for is
//! remembered, so that when it comes up it is told about everything that was opened while it was
//! starting. Without that, a server that takes two seconds would answer nothing about the file
//! that started it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use crate::client::{Client, ClientError, Notice};
use crate::registry::Wanted;

/// Which client this is: a server name and where it is rooted.
pub type Key = (String, PathBuf);

/// What asking for a client found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    /// It is running; use it.
    Running,
    /// It is being started. Ask again later.
    Starting,
    /// It could not be started, and this is why. Not tried again.
    Failed(String),
}

/// Every client, and every one that is on its way.
#[derive(Default)]
pub struct Pool {
    running: BTreeMap<Key, Client>,
    starting: BTreeMap<Key, Vec<PathBuf>>,
    failed: BTreeMap<Key, String>,
}

impl Pool {
    /// An empty pool.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// What is running for `key`.
    #[must_use]
    pub fn get(&self, key: &Key) -> Option<&Client> {
        self.running.get(key)
    }

    /// The same, to talk to.
    pub fn get_mut(&mut self, key: &Key) -> Option<&mut Client> {
        self.running.get_mut(key)
    }

    /// Every client that is running.
    pub fn running(&mut self) -> impl Iterator<Item = &mut Client> {
        self.running.values_mut()
    }

    /// Which key `wanted` would be.
    #[must_use]
    pub fn key_of(wanted: &Wanted) -> Key {
        (wanted.name.clone(), wanted.root.clone())
    }

    /// Whether `wanted` is running, starting, or has failed.
    #[must_use]
    pub fn state(&self, key: &Key) -> Option<Asked> {
        if self.running.contains_key(key) {
            Some(Asked::Running)
        } else if self.starting.contains_key(key) {
            Some(Asked::Starting)
        } else {
            self.failed.get(key).cloned().map(Asked::Failed)
        }
    }

    /// Says that `wanted` is being started for `path`.
    ///
    /// Answers `false` when it is already running, starting or known to have failed — in which
    /// case the caller should not start it again.
    pub fn begin(&mut self, wanted: &Wanted, path: &Path) -> bool {
        let key = Self::key_of(wanted);
        if self.running.contains_key(&key) || self.failed.contains_key(&key) {
            return false;
        }
        match self.starting.get_mut(&key) {
            Some(waiting) => {
                // Already on its way: remember this file too, so it is told about it when it
                // arrives.
                if !waiting.contains(&path.to_path_buf()) {
                    waiting.push(path.to_path_buf());
                }
                false
            }
            None => {
                self.starting.insert(key, vec![path.to_path_buf()]);
                true
            }
        }
    }

    /// Puts a started client in, and answers the files it should be told about.
    pub fn arrived(&mut self, client: Client) -> Vec<PathBuf> {
        let key = (client.name.clone(), client.root.clone());
        let waiting = self.starting.remove(&key).unwrap_or_default();
        self.running.insert(key, client);
        waiting
    }

    /// Records that a client could not be started, so nothing tries again.
    ///
    /// Not retrying is deliberate: a server that is not installed is not going to become installed
    /// while the editor is open, and retrying on every keystroke would be a process spawn per
    /// keystroke.
    pub fn failed(&mut self, wanted: &Wanted, error: &ClientError) {
        let key = Self::key_of(wanted);
        self.starting.remove(&key);
        self.failed.insert(key, error.to_string());
    }

    /// Forgets a client that has gone away, so the next file that wants it starts it again.
    pub fn exited(&mut self, server: &str) {
        self.running.retain(|(name, _), _| name != server);
        self.starting.retain(|(name, _), _| name != server);
    }

    /// Lets a failed server be tried again, which reloading the configuration does.
    pub fn clear_failures(&mut self) {
        self.failed.clear();
    }

    /// Every client, taken out, for shutting down.
    #[must_use]
    pub fn drain(&mut self) -> Vec<Client> {
        self.starting.clear();
        std::mem::take(&mut self.running).into_values().collect()
    }
}

/// Starts `wanted` and hands what it says to `notices`.
///
/// A thin wrapper over [`Client::start`], kept here so a caller has one place to look.
///
/// # Errors
///
/// If the program will not start, or the handshake fails.
pub async fn start(wanted: &Wanted, notices: Sender<Notice>) -> Result<Client, ClientError> {
    Client::start(wanted, notices).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wanted(name: &str, root: &str) -> Wanted {
        Wanted {
            name: name.to_owned(),
            command: name.to_owned(),
            args: Vec::new(),
            root: PathBuf::from(root),
            initialization_options: None,
            settings: None,
            env: BTreeMap::new(),
        }
    }

    #[test]
    fn the_first_ask_starts_it_and_the_second_waits() {
        let mut pool = Pool::new();
        let server = wanted("rust-analyzer", "/project");

        assert!(pool.begin(&server, Path::new("/project/a.rs")));
        assert_eq!(
            pool.state(&Pool::key_of(&server)),
            Some(Asked::Starting),
            "and it says so"
        );
        assert!(
            !pool.begin(&server, Path::new("/project/b.rs")),
            "a second file does not start a second server"
        );
    }

    #[test]
    fn everything_opened_while_it_started_is_remembered() {
        let mut pool = Pool::new();
        let server = wanted("rust-analyzer", "/project");

        pool.begin(&server, Path::new("/project/a.rs"));
        pool.begin(&server, Path::new("/project/b.rs"));
        pool.begin(&server, Path::new("/project/a.rs"));

        // Nothing here can make a real client, so the waiting list is checked directly: it is what
        // `arrived` hands back, and what stops the file that started a server being invisible to
        // it.
        let waiting = pool.starting.get(&Pool::key_of(&server)).expect("waiting");
        assert_eq!(waiting.len(), 2, "each file once");
    }

    #[test]
    fn one_server_at_two_roots_is_two_clients() {
        let mut pool = Pool::new();
        assert!(pool.begin(&wanted("rust-analyzer", "/one"), Path::new("/one/a.rs")));
        assert!(pool.begin(&wanted("rust-analyzer", "/two"), Path::new("/two/a.rs")));
    }

    #[test]
    fn a_server_that_will_not_start_is_not_tried_again() {
        let mut pool = Pool::new();
        let server = wanted("not-installed", "/project");

        pool.begin(&server, Path::new("/project/a.rs"));
        pool.failed(
            &server,
            &ClientError::Protocol("no such program".to_owned()),
        );

        assert!(matches!(
            pool.state(&Pool::key_of(&server)),
            Some(Asked::Failed(_))
        ));
        assert!(
            !pool.begin(&server, Path::new("/project/b.rs")),
            "a process spawn per keystroke is not a retry policy"
        );
    }

    #[test]
    fn reloading_the_configuration_lets_it_be_tried_again() {
        let mut pool = Pool::new();
        let server = wanted("not-installed", "/project");

        pool.begin(&server, Path::new("/project/a.rs"));
        pool.failed(&server, &ClientError::Protocol("nope".to_owned()));
        pool.clear_failures();

        assert_eq!(pool.state(&Pool::key_of(&server)), None);
        assert!(pool.begin(&server, Path::new("/project/a.rs")));
    }

    #[test]
    fn a_server_that_goes_away_can_be_started_again() {
        let mut pool = Pool::new();
        let server = wanted("rust-analyzer", "/project");

        pool.begin(&server, Path::new("/project/a.rs"));
        pool.exited("rust-analyzer");

        assert_eq!(pool.state(&Pool::key_of(&server)), None);
        assert!(pool.begin(&server, Path::new("/project/a.rs")));
    }

    #[test]
    fn nothing_asked_for_is_nothing_known() {
        let pool = Pool::new();
        assert_eq!(
            pool.state(&("never-heard-of".to_owned(), PathBuf::from("/"))),
            None
        );
    }
}
