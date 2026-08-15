//! The language servers, as the interface uses them.
//!
//! [`zdt_lsp`] knows how to talk to a server. This decides when to: which buffer wants one, what
//! to tell it as the text changes, and where the answers go.
//!
//! # The two threads
//!
//! A client is driven on the background runtime and its socket is `Send`; everything on this side
//! is `Rc` and belongs to the interface thread. So the two never share a value: requests go over
//! by cloning the socket into a task, and answers come back either as the task's return value or,
//! for anything the server says unasked, down a channel this drains from a timer.
//!
//! # Versions
//!
//! Every change carries a version, and a diagnostic that names an older one is dropped. Without
//! that, the underline from two keystroke ago lands on text that has moved — which is worse than
//! no underline, because it points at the wrong thing with the same confidence.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use rustc_hash::FxHashMap;
use zdt_lsp::client::Notice;
use zdt_lsp::diagnostics::{Counts, Store};
use zdt_lsp::pool::{Key, Pool};
use zdt_lsp::registry::Wanted;
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

use crate::settings::Settings;
use crate::workspace::{BufferId, Workspace};

/// How long after the last keystroke a server is told what changed.
///
/// Short enough that diagnostics feel live, long enough that typing a word is one notification
/// rather than five.
const SYNC_DEBOUNCE: Duration = Duration::from_millis(120);

/// How often what the servers have said is taken off the channel.
const DRAIN: Duration = Duration::from_millis(50);

/// The language servers.
#[derive(Clone)]
pub struct Language {
    inner: Rc<Inner>,
}

struct Inner {
    workspace: Workspace,
    settings: Settings,
    /// The window's clock, taken once where there certainly is one.
    timers: Option<zgui::view::time::Timers>,

    /// Every client, running or on its way.
    pool: RefCell<Pool>,
    /// What every server has said is wrong.
    store: RefCell<Store>,
    /// Which client each open file is synchronised with.
    files: RefCell<FxHashMap<PathBuf, Vec<Key>>>,
    /// What version each file is at, as the servers have been told.
    versions: RefCell<FxHashMap<PathBuf, i32>>,

    /// What a server has said, waiting to be taken.
    notices: Sender<Notice>,
    inbox: RefCell<Option<Receiver<Notice>>>,
    /// What is draining it, held so that dropping this stops the draining.
    draining: RefCell<Option<zgui::view::time::IntervalHandle>>,
    /// What is waiting to tell a server what changed, by file.
    pending: RefCell<FxHashMap<PathBuf, zgui::view::time::TimeoutHandle>>,

    /// A number that changes whenever anything a view draws has changed.
    ///
    /// One signal rather than one per file: what reads it is the status line and the decorations
    /// of the buffer on screen, and both want to know "something moved" rather than what.
    revision: RwSignal<u64, LocalStorage>,
    /// What the servers are busy with, for the status line.
    busy: RwSignal<Option<String>, LocalStorage>,
    /// Whether servers are wanted at all.
    enabled: Cell<bool>,
}

impl Language {
    /// No servers yet.
    #[must_use]
    pub fn new(workspace: Workspace, settings: Settings) -> Self {
        let (notices, inbox) = std::sync::mpsc::channel();
        let enabled = settings.with_untracked(|config| config.lsp.enabled);

        Self {
            inner: Rc::new(Inner {
                workspace,
                settings,
                timers: zgui::view::time::Timers::current(),
                pool: RefCell::new(Pool::new()),
                store: RefCell::new(Store::new()),
                files: RefCell::new(FxHashMap::default()),
                versions: RefCell::new(FxHashMap::default()),
                notices,
                inbox: RefCell::new(Some(inbox)),
                draining: RefCell::new(None),
                pending: RefCell::new(FxHashMap::default()),
                revision: RwSignal::new_local(0),
                busy: RwSignal::new_local(None),
                enabled: Cell::new(enabled),
            }),
        }
    }

    /// Starts taking what the servers say off the channel.
    ///
    /// Called once, from the root. Until it is, nothing a server says is drawn.
    pub fn listen(&self) {
        let Some(timers) = self.inner.timers.clone() else {
            return;
        };
        let Some(inbox) = self.inner.inbox.borrow_mut().take() else {
            return;
        };
        let language = self.clone();
        let handle = timers.set_interval(DRAIN, move || {
            let mut moved = false;
            for notice in inbox.try_iter() {
                moved |= language.absorb(notice);
            }
            if moved {
                language.touch();
            }
        });
        *self.inner.draining.borrow_mut() = Some(handle);
    }

    // ---- What the interface reads ------------------------------------------------------------

    /// A number that changes whenever anything a view draws has. Tracked.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.inner.revision.get()
    }

    /// What the servers are busy with, when they say. Tracked.
    #[must_use]
    pub fn busy(&self) -> Option<String> {
        self.inner.busy.get()
    }

    /// Everything wrong with `path`.
    #[must_use]
    pub fn diagnostics(&self, path: &Path) -> Vec<lsp_types::Diagnostic> {
        self.inner.store.borrow().for_file(path)
    }

    /// How many of each kind `path` has.
    #[must_use]
    pub fn counts(&self, path: &Path) -> Counts {
        self.inner.store.borrow().counts(path)
    }

    /// The next diagnostic after `line`, wrapping.
    #[must_use]
    pub fn after(&self, path: &Path, line: u32) -> Option<lsp_types::Diagnostic> {
        self.inner.store.borrow().after(path, line)
    }

    /// The one before it, wrapping.
    #[must_use]
    pub fn before(&self, path: &Path, line: u32) -> Option<lsp_types::Diagnostic> {
        self.inner.store.borrow().before(path, line)
    }

    /// Everything on `line`.
    #[must_use]
    pub fn on_line(&self, path: &Path, line: u32) -> Vec<lsp_types::Diagnostic> {
        self.inner.store.borrow().on_line(path, line)
    }

    /// Which servers are answering for `path`.
    #[must_use]
    pub fn servers_for(&self, path: &Path) -> Vec<String> {
        self.inner
            .files
            .borrow()
            .get(path)
            .map(|keys| keys.iter().map(|(name, _)| name.clone()).collect())
            .unwrap_or_default()
    }

    // ---- Keeping up with the buffers ----------------------------------------------------------

    /// Says a buffer has been opened, starting whatever servers claim it.
    pub fn opened(&self, buffer: BufferId) {
        if !self.inner.enabled.get() {
            return;
        }
        let Some((path, language, text)) = self.about(buffer) else {
            return;
        };

        let servers = self.wanted(&language, &path);
        if servers.is_empty() {
            return;
        }

        let mut keys = Vec::new();
        for server in servers {
            let key = Pool::key_of(&server);
            keys.push(key.clone());

            let mut pool = self.inner.pool.borrow_mut();
            if pool.get_mut(&key).is_some() {
                // Already running: tell it about this file now.
                let version = self.next_version(&path);
                if let Some(client) = pool.get_mut(&key) {
                    client.open(&path, &language, version, text.clone());
                }
                continue;
            }
            if !pool.begin(&server, &path) {
                continue;
            }
            drop(pool);
            self.start(server);
        }

        self.inner.files.borrow_mut().insert(path, keys);
    }

    /// Says a buffer's text has changed, after a pause.
    ///
    /// Whole-text rather than incremental: the editor reports what changed, but a rope that is
    /// re-read here is the one thing certain to be what the buffer holds, and the cost of sending
    /// a file is small beside being subtly out of step with a server. Incremental sync is worth
    /// doing when there is a test that can prove it stays in step.
    pub fn changed(&self, buffer: BufferId) {
        if !self.inner.enabled.get() {
            return;
        }
        let Some((path, _, _)) = self.about(buffer) else {
            return;
        };
        let Some(timers) = self.inner.timers.clone() else {
            return;
        };

        let language = self.clone();
        let waiting = path.clone();
        let handle = timers.set_timeout(SYNC_DEBOUNCE, move || {
            language.inner.pending.borrow_mut().remove(&waiting);
            language.send_change(buffer);
        });
        // Replacing the handle cancels the one before it, which is the debounce.
        self.inner.pending.borrow_mut().insert(path, handle);
    }

    /// Says a buffer has been written.
    pub fn saved(&self, buffer: BufferId) {
        if !self.inner.enabled.get() {
            return;
        }
        // Anything still waiting to be sent goes first: a server told about a save before the
        // change that preceded it would lint the version before last.
        self.send_change(buffer);

        let Some((path, _, _)) = self.about(buffer) else {
            return;
        };
        self.with_clients(&path, |client, path| client.save(path, None));
    }

    /// Says a buffer has been closed.
    pub fn closed(&self, path: &Path) {
        self.with_clients(path, |client, path| client.close(path));
        self.inner.files.borrow_mut().remove(path);
        self.inner.versions.borrow_mut().remove(path);
        self.inner.pending.borrow_mut().remove(path);
        self.inner.store.borrow_mut().forget(path);
        self.touch();
    }

    /// Reads the settings again: whether servers are wanted, and which.
    ///
    /// A server that could not be started before is allowed to be tried again, because the reason
    /// it failed may be exactly what was just changed.
    pub fn reconfigure(&self) {
        let enabled = self
            .inner
            .settings
            .with_untracked(|config| config.lsp.enabled);
        self.inner.enabled.set(enabled);
        self.inner.pool.borrow_mut().clear_failures();
        if !enabled {
            self.stop_all();
        }
    }

    /// Shuts every server down.
    pub fn stop_all(&self) {
        let clients = self.inner.pool.borrow_mut().drain();
        self.inner.files.borrow_mut().clear();
        *self.inner.store.borrow_mut() = Store::new();
        self.touch();

        for mut client in clients {
            crate::task::detached(async move {
                if let Err(error) = client.shutdown().await {
                    tracing::debug!("{}: {error}", client.name);
                }
            });
        }
    }

    // ---- Asking ------------------------------------------------------------------------------

    /// The client answering for `path`, cloned so it can be taken to a worker.
    ///
    /// The first one, when several answer: a request has one answer, and a person pressing `gd`
    /// means the definition rather than every server's opinion of it.
    #[must_use]
    pub fn client_for(&self, path: &Path) -> Option<zdt_lsp::Client> {
        let files = self.inner.files.borrow();
        let keys = files.get(path)?;
        let pool = self.inner.pool.borrow();
        keys.iter().find_map(|key| pool.get(key).cloned())
    }

    // ---- Internals ---------------------------------------------------------------------------

    /// Starts a server, and tells it about everything that was waiting for it.
    fn start(&self, wanted: Wanted) {
        let language = self.clone();
        let notices = self.inner.notices.clone();
        crate::task::detached(async move {
            let started = {
                let wanted = wanted.clone();
                zgui::task::background(async move { zdt_lsp::pool::start(&wanted, notices).await })
                    .await
            };

            match started {
                Ok(client) => {
                    let waiting = language.inner.pool.borrow_mut().arrived(client);
                    for path in waiting {
                        language.open_now(&wanted, &path);
                    }
                    language
                        .inner
                        .workspace
                        .say(format!("{} is ready", wanted.name));
                }
                Err(error) => {
                    language.inner.pool.borrow_mut().failed(&wanted, &error);
                    language.inner.workspace.complain(error.to_string());
                }
            }
            language.touch();
        });
    }

    /// Tells a just-started server about a file that was open before it was.
    fn open_now(&self, wanted: &Wanted, path: &Path) {
        let Some(buffer) = self.inner.workspace.find_path(path) else {
            return;
        };
        let Some((path, language, text)) = self.about(buffer) else {
            return;
        };
        let version = self.next_version(&path);
        let key = Pool::key_of(wanted);
        if let Some(client) = self.inner.pool.borrow_mut().get_mut(&key) {
            client.open(&path, &language, version, text);
        }
    }

    /// Tells every server answering for a buffer what it now holds.
    fn send_change(&self, buffer: BufferId) {
        let Some((path, _, text)) = self.about(buffer) else {
            return;
        };
        let version = self.next_version(&path);
        self.with_clients(&path, |client, path| {
            client.change(
                path,
                version,
                vec![lsp_types::TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: text.clone(),
                }],
            );
        });
    }

    /// Runs `act` against every client answering for `path`.
    fn with_clients(&self, path: &Path, mut act: impl FnMut(&mut zdt_lsp::Client, &Path)) {
        let keys = self.inner.files.borrow().get(path).cloned();
        let Some(keys) = keys else {
            return;
        };
        let mut pool = self.inner.pool.borrow_mut();
        for key in keys {
            if let Some(client) = pool.get_mut(&key) {
                act(client, path);
            }
        }
    }

    /// The path, language and text of a buffer, when it has all three.
    fn about(&self, buffer: BufferId) -> Option<(PathBuf, String, String)> {
        let entry = self.inner.workspace.buffer_untracked(buffer)?;
        let path = entry.path.clone()?;
        let language = entry.language()?.to_owned();
        let text = entry.document()?.text();
        Some((path, language, text))
    }

    /// Which servers claim a file.
    fn wanted(&self, language: &str, path: &Path) -> Vec<Wanted> {
        let root = self.inner.workspace.project().root().to_path_buf();
        self.inner.settings.with_untracked(|config| {
            zdt_lsp::registry::wanted_for(&config.lsp.servers, language, path, &root)
        })
    }

    /// The next version number for a file. Every change gets one, and it only goes up.
    fn next_version(&self, path: &Path) -> i32 {
        let mut versions = self.inner.versions.borrow_mut();
        let version = versions.entry(path.to_path_buf()).or_insert(0);
        *version += 1;
        *version
    }

    /// Takes one thing a server said. Answers whether anything a view draws has changed.
    fn absorb(&self, notice: Notice) -> bool {
        match notice {
            Notice::Diagnostics {
                uri,
                diagnostics,
                version,
            } => {
                let Some(path) = zdt_lsp::convert::path_of(&uri) else {
                    return false;
                };
                // A diagnostic about a version that has been typed past points at text that has
                // moved. Dropping it is better than drawing it in the wrong place.
                if let Some(version) = version
                    && let Some(current) = self.inner.versions.borrow().get(&path)
                    && version < *current
                {
                    return false;
                }
                // Which server said it: the first one answering for the file. A publish carries
                // no server name, so this is the best that can be known, and it is right whenever
                // one server answers for a file — which is almost always.
                let server = self
                    .servers_for(&path)
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "lsp".to_owned());
                self.inner
                    .store
                    .borrow_mut()
                    .set(&path, &server, diagnostics);
                true
            }
            Notice::Message {
                server,
                severity,
                text,
            } => {
                let text = format!("{server}: {text}");
                if severity == lsp_types::MessageType::ERROR {
                    self.inner.workspace.complain(text);
                } else {
                    self.inner.workspace.say(text);
                }
                false
            }
            Notice::Progress {
                server,
                title,
                done,
            } => {
                let now = if done {
                    None
                } else {
                    Some(match title {
                        Some(title) => format!("{server}: {title}"),
                        None => server,
                    })
                };
                if self.inner.busy.get_untracked() != now {
                    self.inner.busy.set(now);
                }
                false
            }
            Notice::Exited { server } => {
                self.inner.pool.borrow_mut().exited(&server);
                self.inner.store.borrow_mut().forget_server(&server);
                self.inner
                    .workspace
                    .complain(format!("{server} has stopped"));
                true
            }
        }
    }

    /// Says that something a view draws has changed.
    fn touch(&self) {
        self.inner
            .revision
            .update(|revision| *revision = revision.wrapping_add(1));
    }
}

/// Puts the language layer where every component can find it.
pub fn provide(language: Language) {
    zgui::reactive::provide_local_context(language);
}

/// It, from inside a component.
///
/// # Panics
///
/// If none was provided above this component, which is a wiring mistake.
#[must_use]
pub fn use_language() -> Language {
    zgui::reactive::use_local_context::<Language>().expect("a language layer is at the root")
}
