//! The language servers, as the interface uses them.
//!
//! [`zdt_lsp`] knows how to talk to a server. This decides when to: which buffer wants one, what
//! to tell it as the text changes, and where the answers go.
//!
//! # The two threads
//!
//! A client is driven on the background runtime and its socket is `Send`. Everything on this side
//! is `Rc` and belongs to the interface thread. So the two never share a value. Requests go over
//! by cloning the socket into a task. Answers come back as the task's return value, or, for
//! anything the server says unasked, down a channel this drains from a timer.
//!
//! # Versions
//!
//! Every change carries a version, and a diagnostic that names an older one is dropped. Without
//! that, the underline from two keystrokes ago lands on text that has moved. That is worse than no
//! underline, because it points at the wrong thing with the same confidence.

pub mod diagnostics;

mod notices;
mod read;
mod sync;

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
use zgui_ui::toast::{Toast, ToastKind};

use crate::settings::Settings;
use crate::workspace::{BufferId, Workspace};

/// How long after the last keystroke a server is told what changed.
///
/// Short enough that diagnostics feel live, and long enough that typing a word is one
/// notification.
const SYNC_DEBOUNCE: Duration = Duration::from_millis(120);

/// How often what the servers have said is taken off the channel.
const DRAIN: Duration = Duration::from_millis(50);

/// What the servers answering for a file are doing.
///
/// One word for the status line. The status line says *state*, meaning what things are, which
/// stays true until it changes. Announcements say *events*, which are true once. Mixing them makes
/// a status line that flickers and a stack of toasts nobody reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ServerState {
    /// Nothing claims this file, or servers are switched off altogether.
    #[default]
    Inactive,
    /// One is on its way up.
    Starting,
    /// One is working through the project.
    Indexing,
    /// One is up and idle.
    Ready,
    /// One could not be started, and is not being tried again.
    Failed,
}

impl ServerState {
    /// The word the status line shows.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Inactive => "inactive",
            Self::Starting => "starting",
            Self::Indexing => "indexing",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }

    /// How this is written as an attribute value, which is what the style sheet colours by.
    #[must_use]
    pub const fn tone(self) -> &'static str {
        self.label()
    }

    /// Whether it is worth turning a spinner for.
    #[must_use]
    pub const fn is_working(self) -> bool {
        matches!(self, Self::Starting | Self::Indexing)
    }
}

/// The language servers.
#[derive(Clone)]
pub struct Language {
    inner: Rc<Inner>,
}

struct Inner {
    workspace: Workspace,
    settings: Settings,
    /// Where a server's news goes.
    ///
    /// Taken once at construction. Most of what is announced here happens inside a task's
    /// continuation or a timer's callback, and neither runs inside the scope that has one.
    notify: Option<crate::notify::Notify>,
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
    /// One signal for everything, and never one per file. The status line and the decorations of
    /// the buffer on screen read it, and both want to know that something moved.
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
                notify: crate::notify::use_notify(),
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

    /// Where anything a server says unasked is posted.
    ///
    /// Cloned into each client as it starts. Public so that what a server would say can be posted
    /// without starting one, which is the only way to assert what a flood of progress does.
    #[must_use]
    pub fn notices(&self) -> Sender<Notice> {
        self.inner.notices.clone()
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
}

/// How much a state is worth saying, higher winning.
///
/// A failure beats work in progress beats readiness, because that is the order somebody would want
/// to hear them in: the one that needs doing something about comes first.
const fn rank(state: ServerState) -> u8 {
    match state {
        ServerState::Inactive => 0,
        ServerState::Ready => 1,
        ServerState::Indexing => 2,
        ServerState::Starting => 3,
        ServerState::Failed => 4,
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
