//! Announcements from something that outlives the window they appear in.
//!
//! A [`Notify`](super::Notify) is one window's stack of toasts. Anything longer-lived — a session,
//! or a language server it started — cannot hold one: the window it was taken from closes, and the
//! next window's stack is a different one.
//!
//! An `Announcer` is the seam. It is bound to whichever window is looking at the thing announcing,
//! and holds what was said while none was, so a server that finished indexing behind a closed
//! window still says so when one comes back.

use std::cell::RefCell;
use std::rc::Rc;

use super::Notify;

/// How much is held for a window that is not there.
///
/// Small on purpose. A backlog is for the announcement somebody missed by a second, and not a log.
const BACKLOG: usize = 16;

/// One thing to say.
#[derive(Clone, Debug)]
enum Said {
    Say(String),
    Ok(String),
    Warn(String),
    Fail(String, Option<String>),
}

/// Where something says what happened, whichever window is listening.
///
/// Cloning one is cloning a handle.
#[derive(Clone, Default)]
pub struct Announcer {
    inner: Rc<Inner>,
}

#[derive(Default)]
struct Inner {
    /// The window that is listening, while one is.
    current: RefCell<Option<Notify>>,
    /// What was said while none was.
    backlog: RefCell<std::collections::VecDeque<Said>>,
}

impl Announcer {
    /// An announcer with nobody listening yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Says announcements go to `notify` now, and repeats what was missed.
    pub fn bind(&self, notify: Notify) {
        let missed: Vec<Said> = self.inner.backlog.borrow_mut().drain(..).collect();
        *self.inner.current.borrow_mut() = Some(notify.clone());
        for said in missed {
            Self::deliver(&notify, said);
        }
    }

    /// Says nobody is listening. What is said from now on is held.
    pub fn unbind(&self) {
        self.inner.current.borrow_mut().take();
    }

    /// Whether a window is listening.
    #[must_use]
    pub fn is_bound(&self) -> bool {
        self.inner.current.borrow().is_some()
    }

    /// The window's own stack, when one is listening.
    ///
    /// For the few things that need a `Notify` itself: a keyed progress row cannot be held back,
    /// because by the time a window arrives the job it was about has finished.
    #[must_use]
    pub fn notify(&self) -> Option<Notify> {
        self.inner.current.borrow().clone()
    }

    /// Something happened.
    pub fn say(&self, title: impl Into<String>) {
        self.push(Said::Say(title.into()));
    }

    /// Something worked.
    pub fn ok(&self, title: impl Into<String>) {
        self.push(Said::Ok(title.into()));
    }

    /// Something is not right.
    pub fn warn(&self, title: impl Into<String>) {
        self.push(Said::Warn(title.into()));
    }

    /// Something went wrong.
    pub fn fail(&self, title: impl Into<String>, detail: Option<String>) {
        self.push(Said::Fail(title.into(), detail));
    }

    /// Says something under `key`, replacing whatever was there.
    ///
    /// Never held back. A keyed row is about work in progress, and by the time a window opens the
    /// work it described has finished, so a backlogged one would announce the past.
    pub fn progress(&self, key: &str, toast: zgui_ui::toast::Toast) {
        if let Some(notify) = self.inner.current.borrow().as_ref() {
            notify.progress(key, toast);
        }
    }

    /// Gives the row under `key` back.
    pub fn clear(&self, key: &str) {
        if let Some(notify) = self.inner.current.borrow().as_ref() {
            notify.clear(key);
        }
    }

    /// To the window that is listening, or onto the backlog.
    fn push(&self, said: Said) {
        match self.inner.current.borrow().as_ref() {
            Some(notify) => Self::deliver(notify, said),
            None => {
                let mut backlog = self.inner.backlog.borrow_mut();
                if backlog.len() == BACKLOG {
                    backlog.pop_front();
                }
                backlog.push_back(said);
            }
        }
    }

    /// One announcement, onto one window's stack.
    fn deliver(notify: &Notify, said: Said) {
        match said {
            Said::Say(title) => notify.say(title),
            Said::Ok(title) => notify.ok(title),
            Said::Warn(title) => notify.warn(title),
            Said::Fail(title, detail) => notify.fail(title, detail),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Announcer, BACKLOG, Said};

    #[test]
    fn nothing_is_lost_while_no_window_is_listening() {
        let announcer = Announcer::new();
        assert!(!announcer.is_bound());
        announcer.say("indexing finished");
        assert_eq!(announcer.inner.backlog.borrow().len(), 1);
    }

    #[test]
    fn the_backlog_is_bounded() {
        let announcer = Announcer::new();
        for step in 0..BACKLOG * 2 {
            announcer.say(format!("{step}"));
        }
        assert_eq!(announcer.inner.backlog.borrow().len(), BACKLOG);
        // The oldest go first: what somebody missed a moment ago is worth more than what they
        // missed a minute ago.
        let first = announcer.inner.backlog.borrow().front().cloned();
        assert!(matches!(first, Some(Said::Say(ref title)) if title == &BACKLOG.to_string()));
    }

    #[test]
    fn unbinding_holds_what_comes_next() {
        let announcer = Announcer::new();
        announcer.unbind();
        announcer.fail("git", Some("no repository".to_owned()));
        assert_eq!(announcer.inner.backlog.borrow().len(), 1);
        assert!(announcer.notify().is_none());
    }
}
