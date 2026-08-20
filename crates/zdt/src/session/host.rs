//! Every session there is, and every window looking at one.
//!
//! The registry. One session per directory, for the life of the application: asking for a
//! directory that is already open answers with the session that is already there, which is what
//! makes "open this project" mean the same thing however it is asked for.
//!
//! Built in the application's scope, above every window, so that a window closing takes no
//! session with it.

use std::cell::RefCell;
use std::rc::Rc;

use rustc_hash::FxHashMap;
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, Owner, RwSignal};

use crate::app::RootProps;
use crate::app::global::Global;
use crate::session::client::{Client, ClientId};
use crate::session::{Session, SessionId, SessionKey};

/// How long a killed session is left alone before its signals go.
///
/// Long enough for the views that were drawing it to come down, which happens on the next
/// reactive flush.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(120);

/// What asking for a directory came to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Revealed {
    /// It was already on screen, and that window was brought forward.
    Focused,
    /// It was already open but nowhere on screen, and a window now shows it.
    Shown,
    /// There was no such session, and there is now.
    Opened,
}

/// One session in the registry, as something outside it can read.
#[derive(Clone, Debug)]
pub struct Listed {
    pub id: SessionId,
    pub key: SessionKey,
    /// What it is called.
    pub name: String,
    /// How many buffers are open in it.
    pub buffers: usize,
    /// Whether a window is looking at it.
    pub attached: bool,
}

/// Every session, and every window.
///
/// Cloning one is cloning a handle.
#[derive(Clone)]
pub struct SessionHost {
    inner: Rc<Inner>,
}

struct Inner {
    global: Global,
    /// The application's own scope. Every session hangs off a child of it.
    owner: Owner,
    sessions: RefCell<slotmap::SlotMap<SessionId, Session>>,
    /// Which session each directory is, so one directory is never two sessions.
    by_key: RefCell<FxHashMap<SessionKey, SessionId>>,
    clients: RefCell<slotmap::SlotMap<ClientId, Client>>,
    /// The clock the control socket's queue is drained on.
    ///
    /// The registry's own, lent by whichever window is open: it is older than any of them, and
    /// the queue has to be looked at whichever one that is.
    clock: zdt_view::Clock,
    /// Counts up whenever the set of sessions changes, so a picker listing them redraws.
    revision: RwSignal<u64, LocalStorage>,
}

/// A reactive owner with nothing above it.
///
/// What [`SessionHost::new`] wants, and the one thing that has to be made in the right place: an
/// owner takes the current one as its parent, and cleaning a parent cleans every child. A session
/// under a window's owner therefore loses every signal it has the moment that window closes,
/// silently — the buffers read as defaults rather than failing.
///
/// So this is called before anything has set an owner, which in practice means from `main` after
/// [`zgui::reactive::install`] and before the application runs.
///
/// # Panics
///
/// In debug builds, if something already owns this scope, because the answer would not be a root.
pub fn detached_root() -> Owner {
    debug_assert!(
        Owner::current().is_none(),
        "a session host's owner must have nothing above it; make it before the application runs",
    );
    Owner::new()
}

impl SessionHost {
    /// An empty registry, with every session hanging off a child of `owner`.
    ///
    /// `owner` must outlive every window: see [`detached_root`], which is how to get one.
    #[must_use]
    pub fn new(global: Global, owner: Owner) -> Self {
        debug_assert!(
            owner.parent().is_none(),
            "a session host's owner must have nothing above it, or sessions die with a window",
        );
        Self {
            inner: Rc::new(Inner {
                global,
                owner,
                sessions: RefCell::new(slotmap::SlotMap::with_key()),
                by_key: RefCell::new(FxHashMap::default()),
                clients: RefCell::new(slotmap::SlotMap::with_key()),
                clock: zdt_view::Clock::new(),
                revision: RwSignal::new_local(0),
            }),
        }
    }

    /// The session for `key`, made if there is none.
    pub fn open(&self, key: SessionKey) -> SessionId {
        if let Some(id) = self.inner.by_key.borrow().get(&key) {
            return *id;
        }
        // A child of the application's scope, so the session outlives every window.
        let owner = self.inner.owner.child();
        let global = self.inner.global.clone();
        let id = self
            .inner
            .sessions
            .borrow_mut()
            .insert_with_key(|id| Session::build(id, key.clone(), &global, owner.clone()));
        self.inner.by_key.borrow_mut().insert(key, id);
        self.changed();
        id
    }

    /// The session under `id`, when there is one.
    #[must_use]
    pub fn session(&self, id: SessionId) -> Option<Session> {
        self.inner.sessions.borrow().get(id).cloned()
    }

    /// Which session `key` is, when it is open.
    #[must_use]
    pub fn find(&self, key: &SessionKey) -> Option<SessionId> {
        self.inner.by_key.borrow().get(key).copied()
    }

    /// Every session, most recently opened last. Tracked.
    #[must_use]
    pub fn list(&self) -> Vec<Listed> {
        let _ = self.inner.revision.get();
        self.list_untracked()
    }

    /// The same, without subscribing.
    #[must_use]
    pub fn list_untracked(&self) -> Vec<Listed> {
        self.inner
            .sessions
            .borrow()
            .iter()
            .map(|(id, session)| Listed {
                id,
                key: session.key().clone(),
                name: session.name(),
                buffers: session.workspace().order().len(),
                attached: session.is_attached(),
            })
            .collect()
    }

    /// Counts up whenever the set of sessions changes. Tracked.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.inner.revision.get()
    }

    /// Puts the session for `key` in a window of its own, and opens `files` in it.
    ///
    /// A window that already holds it is brought forward rather than a second one opened: one
    /// window looks at a session at a time, because a split's editor is registered against the
    /// split and two subtrees over one workspace would each claim the same registration.
    pub fn reveal_in_new_window(&self, key: SessionKey, files: &[std::path::PathBuf]) -> Revealed {
        let existed = self.find(&key).is_some();
        let id = self.open(key);
        self.open_files(id, files);

        if let Some(client) = self.client_holding(id) {
            client.show(id);
            client.focus();
            return Revealed::Focused;
        }
        self.open_client(Some(id));
        if existed {
            Revealed::Shown
        } else {
            Revealed::Opened
        }
    }

    /// Opens `files` in the session under `id`.
    fn open_files(&self, id: SessionId, files: &[std::path::PathBuf]) {
        let Some(session) = self.session(id) else {
            return;
        };
        for file in files {
            crate::files::open_argument(session.workspace(), file);
        }
    }

    /// Puts the session for `key` on screen, wherever it already is, and opens `files` in it.
    pub fn reveal(&self, key: SessionKey, files: &[std::path::PathBuf]) -> Revealed {
        let existed = self.find(&key).is_some();
        let id = self.open(key);
        self.open_files(id, files);

        match self.client_holding(id) {
            Some(client) => {
                let showed = client.showing_untracked() == Some(id);
                client.show(id);
                client.focus();
                if showed {
                    Revealed::Focused
                } else {
                    Revealed::Shown
                }
            }
            None => {
                if let Some(client) = self.any_client() {
                    self.show_in(&client, id);
                    client.focus();
                }
                if existed {
                    Revealed::Shown
                } else {
                    Revealed::Opened
                }
            }
        }
    }

    /// Shows `session` in `client`, unmounting whatever had to make room.
    pub fn show_in(&self, client: &Client, session: SessionId) {
        if let Some(evicted) = client.show(session) {
            tracing::debug!("session {evicted:?} unmounted to make room");
        }
    }

    /// Which window is holding `session`, when one is.
    #[must_use]
    pub fn client_holding(&self, session: SessionId) -> Option<Client> {
        self.inner
            .clients
            .borrow()
            .values()
            .find(|client| client.holds(session))
            .cloned()
    }

    /// Any window at all, for a session that has to go somewhere.
    #[must_use]
    pub fn any_client(&self) -> Option<Client> {
        self.inner.clients.borrow().values().next().cloned()
    }

    /// Every window there is.
    #[must_use]
    pub fn clients(&self) -> Vec<Client> {
        self.inner.clients.borrow().values().cloned().collect()
    }

    /// Opens a window of its own, showing `initial`.
    ///
    /// The attributes are repeated from [`crate::app::window::options`] because a window opened
    /// after the first inherits only the application's stylesheet: without them the desktop draws
    /// a title bar over the one the frame has already drawn.
    pub fn open_client(&self, initial: Option<SessionId>) {
        let Some(windows) = zgui::reactive::use_local_context::<zgui::runtime::windows::Windows>()
        else {
            tracing::warn!("no window service; cannot open a second window");
            return;
        };
        let session = initial.and_then(|id| self.session(id));
        let Some(session) = session.or_else(|| self.first_session()) else {
            return;
        };
        let title = crate::app::window::title_for(&session.name());
        let opening = session.clone();
        // Under the registry's own owner, and never under whatever scope asked for the window.
        // A `WindowHandle` holds signals made where it was asked for, and asking from inside an
        // event handler leaves them owned by a scope that is disposed of before the window has
        // finished opening — which aborts the process when the runtime next reads one.
        self.inner.owner.with(|| {
            windows.open(crate::app::window::options(&title), move || {
                zgui::view! { Root(session = opening.clone(), files = Vec::new()) }
            });
        });
    }

    /// The clock the registry's own repeating work runs on.
    #[must_use]
    pub fn clock(&self) -> &zdt_view::Clock {
        &self.inner.clock
    }

    /// The scope every session hangs off, for work that must outlive every window.
    pub fn owner(&self) -> Owner {
        self.inner.owner.clone()
    }

    /// Any session at all, for a window that has to show something.
    #[must_use]
    pub fn first_session(&self) -> Option<Session> {
        self.inner.sessions.borrow().values().next().cloned()
    }

    /// Notes a window, and answers what it is called.
    pub fn register_client(&self, handle: Option<zgui::runtime::windows::WindowHandle>) -> Client {
        let mut clients = self.inner.clients.borrow_mut();
        let id = clients.insert_with_key(|id| Client::new(id, handle.clone()));
        clients[id].clone()
    }

    /// Forgets a window, which closing one does.
    pub fn forget_client(&self, id: ClientId) {
        self.inner.clients.borrow_mut().remove(id);
    }

    /// Takes a session away, stopping its servers and its programs.
    ///
    /// The last session cannot be killed: the editor is always in one.
    pub fn kill(&self, id: SessionId) -> bool {
        if self.inner.sessions.borrow().len() <= 1 {
            return false;
        }
        let Some(session) = self.session(id) else {
            return false;
        };
        // Out of the registry first, so nothing finds it again, and off every window's list, so
        // its subtree comes down.
        for client in self.clients() {
            client.drop_session(id);
        }
        self.inner.by_key.borrow_mut().remove(session.key());
        self.inner.sessions.borrow_mut().remove(id);
        self.changed();

        // And disposed of a moment later. Taking a window's list apart is what unmounts the
        // views, and that happens on the next flush: disposing the signals now would leave those
        // views reading values that are already gone, which is a panic and not a message.
        let owed = self.inner.clock.after(SETTLE, move || session.dispose());
        std::mem::forget(owed);
        true
    }

    /// Writes every session down, now.
    ///
    /// What quitting and closing a window both do. Synchronous, and bounded by the caps the
    /// writer applies: a session is a few hundred kilobytes.
    pub fn flush_all(&self) {
        for session in self.inner.sessions.borrow().values() {
            session.flush();
        }
    }

    /// Says the set of sessions changed.
    fn changed(&self) {
        self.inner.revision.update(|held| *held += 1);
    }
}

/// Publishes `host` to every window.
pub fn provide(host: SessionHost) {
    zgui::reactive::provide_local_context(host);
}

/// The registry, from inside a component.
///
/// # Panics
///
/// If none was installed above this window. That is a wiring mistake, and nothing can carry on
/// from it.
#[must_use]
pub fn use_host() -> SessionHost {
    zgui::reactive::use_local_context::<SessionHost>()
        .expect("a session host is installed at the root")
}
