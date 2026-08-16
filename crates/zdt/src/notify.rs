//! What the editor announces.
//!
//! The status line has one slot, and a slot is a place where the last thing said replaces the one
//! before it. That is right for a reply — `:w` saying "written", `gr` saying "3 references" — and
//! wrong for news: a server failing to start while another is indexing is two pieces of news, and
//! a slot shows one of them.
//!
//! So there are two channels, and the difference between them is not urgency but *shape*:
//!
//!   * a **reply** goes to [`Workspace::say`](crate::workspace::Workspace::say). It answers a key
//!     that was just pressed, it is one line, and it is worth exactly as long as it takes to read.
//!   * an **announcement** comes here. Nobody asked for it, it arrived on its own, and it has to
//!     wait to be read rather than be replaced by the next thing that happens.
//!
//! # Keyed announcements
//!
//! A language server says "indexing" and later says it has finished. Those are one piece of news
//! twice, not two, and a stack that showed both would be a stack that grows while nothing changes.
//! [`Notify::progress`] takes a key and replaces whatever is already under it, so a server owns one
//! row for its whole life; [`Notify::clear`] gives the row back.
//!
//! The queue itself has no update-in-place — replacing is dismissing and pushing — which is why the
//! identifiers are held here rather than being worked out from what is on screen.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use rustc_hash::FxHashMap;
use zgui_ui::toast::{Toast, ToastId, ToastKind, ToastQueue};

/// How long an announcement stays when nothing says otherwise.
///
/// Read from the settings; this is what is used before they have been read, and when the value in
/// them is nonsense.
const DEFAULT_TIMEOUT: Duration = Duration::from_millis(4000);

/// The announcements.
///
/// Cloning one is cloning a handle: every clone announces into the same stack.
#[derive(Clone)]
pub struct Notify {
    inner: Rc<Inner>,
}

struct Inner {
    /// Where announcements go, when a toaster was found above this.
    ///
    /// `None` in a test that mounted a component without one, which is an ordinary state rather
    /// than a mistake: everything below still runs, and says nothing.
    queue: Option<ToastQueue>,
    /// Which announcement is under each key.
    keyed: RefCell<FxHashMap<String, ToastId>>,
    /// The settings, for how long an announcement stays and whether to make one at all.
    settings: crate::settings::Settings,
}

impl Notify {
    /// The announcements of the toaster above this component, when there is one.
    ///
    /// Built inside the toaster rather than beside it, because the queue reaches its callers
    /// through the scope tree and a scope tree only goes downwards.
    #[must_use]
    pub fn new(settings: crate::settings::Settings) -> Self {
        Self {
            inner: Rc::new(Inner {
                queue: zgui_ui::toast::use_toaster(),
                keyed: RefCell::new(FxHashMap::default()),
                settings,
            }),
        }
    }

    /// Something happened.
    pub fn say(&self, title: impl Into<String>) {
        self.push(self.timed(Toast::new(title)));
    }

    /// Something worked.
    pub fn ok(&self, title: impl Into<String>) {
        self.push(self.timed(Toast::new(title).kind(ToastKind::Success)));
    }

    /// Something is worth knowing before it becomes a problem.
    pub fn warn(&self, title: impl Into<String>) {
        self.push(self.timed(Toast::new(title).kind(ToastKind::Warning)));
    }

    /// Something went wrong.
    ///
    /// Persistent, unlike everything else here: a failure that scrolled away before it was read is
    /// a failure that will be reported again as a bug in something else.
    pub fn fail(&self, title: impl Into<String>, detail: Option<String>) {
        let mut toast = Toast::new(title).kind(ToastKind::Error).persistent();
        if let Some(detail) = detail {
            toast = toast.description(detail);
        }
        self.push(toast);
    }

    /// The announcement under `key`, replacing whatever was there.
    ///
    /// What a long-running job uses: one row for the job rather than one row per thing it says
    /// about itself.
    pub fn progress(&self, key: &str, toast: Toast) -> Option<ToastId> {
        self.clear(key);
        let id = self.push(toast)?;
        self.inner.keyed.borrow_mut().insert(key.to_owned(), id);
        Some(id)
    }

    /// Takes the announcement under `key` away, if there is one.
    pub fn clear(&self, key: &str) {
        let held = self.inner.keyed.borrow_mut().remove(key);
        if let (Some(queue), Some(id)) = (self.inner.queue, held) {
            queue.dismiss(id);
        }
    }

    /// Takes every announcement away, which `<Leader>uD` does.
    pub fn dismiss_all(&self) {
        self.inner.keyed.borrow_mut().clear();
        if let Some(queue) = self.inner.queue {
            queue.clear();
        }
    }

    /// How many are on the screen, the ones on their way out included. For the tests.
    #[must_use]
    pub fn showing(&self) -> usize {
        self.inner.queue.map_or(0, |queue| queue.showing().len())
    }

    /// How many are staying. For the tests.
    #[must_use]
    pub fn live(&self) -> usize {
        self.inner.queue.map_or(0, |queue| queue.live().len())
    }

    /// Puts one on the stack, when announcements are wanted at all.
    fn push(&self, toast: Toast) -> Option<ToastId> {
        if !self.wanted() {
            return None;
        }
        self.inner.queue.map(|queue| queue.push(toast))
    }

    /// Whether announcements are wanted.
    fn wanted(&self) -> bool {
        self.inner
            .settings
            .with_untracked(|config| config.ui.notifications)
    }

    /// `toast` with the settings' timeout on it.
    ///
    /// A timeout of zero means "until it is dismissed", which is the only sensible reading of
    /// asking for an announcement that stays for no time at all.
    fn timed(&self, toast: Toast) -> Toast {
        let milliseconds = self
            .inner
            .settings
            .with_untracked(|config| config.ui.notification_timeout);
        match milliseconds {
            0 => toast.persistent(),
            milliseconds => toast.duration(Duration::from_millis(milliseconds)),
        }
    }
}

/// The default timeout, for the settings schema to name.
#[must_use]
pub const fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT.as_millis() as u64
}

/// Puts the announcements where every component can find them.
pub fn provide(notify: Notify) {
    zgui::reactive::provide_local_context(notify);
}

/// Them, from inside a component.
///
/// Unlike the workspace and the settings, this answers `None` rather than panicking: announcing
/// something is not a thing any component depends on being able to do, and a test that mounts one
/// component without a toaster over it should not fail for want of somewhere to put a message.
#[must_use]
pub fn use_notify() -> Option<Notify> {
    zgui::reactive::use_local_context::<Notify>()
}

/// Announces `title`, if there is anywhere to announce it.
///
/// The shorthand the call sites use, so that announcing something is one line rather than three.
pub fn say(title: impl Into<String>) {
    if let Some(notify) = use_notify() {
        notify.say(title);
    }
}

/// The same, for something that worked.
pub fn ok(title: impl Into<String>) {
    if let Some(notify) = use_notify() {
        notify.ok(title);
    }
}

/// The same, for something worth knowing.
pub fn warn(title: impl Into<String>) {
    if let Some(notify) = use_notify() {
        notify.warn(title);
    }
}

/// The same, for something that went wrong.
pub fn fail(title: impl Into<String>, detail: Option<String>) {
    if let Some(notify) = use_notify() {
        notify.fail(title, detail);
    }
}
