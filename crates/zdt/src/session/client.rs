//! One window, and the sessions it holds.
//!
//! tmux's word, and deliberately not "window": [`crate::workspace::WindowId`] already means a
//! split, following vim, and renaming that would be renaming the thing every key is documented
//! against. A client is one operating-system window.
//!
//! A client shows one session and *holds* several. Every session it has visited stays mounted and
//! all but one are taken out of the flow, which is the same trick a pane plays on its buffers
//! (see [`crate::workspace::pane`]) and for the same reason: a terminal taken out of the tree is
//! a program shut down, and an editor taken out of the tree loses its scroll and its selections.
//!
//! Held sessions are capped. Evicting one unmounts its subtree, which stops its programs; its
//! buffers, its layout and its servers live on, because those belong to the session and not to
//! the window.

use std::rc::Rc;

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::runtime::windows::WindowHandle;

use crate::session::SessionId;

slotmap::new_key_type! {
    /// Names one window for as long as the application runs.
    pub struct ClientId;
}

/// How many sessions one window keeps mounted.
///
/// Each one costs its editors, its tree and its git panel, all built and all hidden. Enough that
/// going back and forth between two or three projects is instant, and few enough that a day of
/// hopping does not fill the memory.
pub const HELD_PER_CLIENT: usize = 4;

/// One operating-system window.
///
/// Cloning one is cloning a handle.
#[derive(Clone)]
pub struct Client {
    inner: Rc<Inner>,
}

struct Inner {
    id: ClientId,
    handle: Option<WindowHandle>,
    /// Which session is on screen.
    showing: RwSignal<Option<SessionId>, LocalStorage>,
    /// Every session whose subtree is present here, most recently shown last.
    ///
    /// A session appears in exactly one client's list, which is what keeps a split's editor
    /// registered once rather than twice.
    held: RwSignal<Vec<SessionId>, LocalStorage>,
}

impl Client {
    /// A window that is showing nothing yet.
    #[must_use]
    pub fn new(id: ClientId, handle: Option<WindowHandle>) -> Self {
        Self {
            inner: Rc::new(Inner {
                id,
                handle,
                showing: RwSignal::new_local(None),
                held: RwSignal::new_local(Vec::new()),
            }),
        }
    }

    /// Which window this is.
    #[must_use]
    pub fn id(&self) -> ClientId {
        self.inner.id
    }

    /// The window itself, when this client has one.
    #[must_use]
    pub fn handle(&self) -> Option<&WindowHandle> {
        self.inner.handle.as_ref()
    }

    /// Which session is on screen. Tracked.
    #[must_use]
    pub fn showing(&self) -> Option<SessionId> {
        self.inner.showing.get()
    }

    /// The same, without subscribing.
    #[must_use]
    pub fn showing_untracked(&self) -> Option<SessionId> {
        self.inner.showing.get_untracked()
    }

    /// Every session mounted here, oldest first. Tracked.
    #[must_use]
    pub fn held(&self) -> Vec<SessionId> {
        self.inner.held.get()
    }

    /// Whether `session` is mounted here.
    #[must_use]
    pub fn holds(&self, session: SessionId) -> bool {
        self.inner
            .held
            .with_untracked(|held| held.contains(&session))
    }

    /// Puts `session` on screen, mounting it first when it is not here yet.
    ///
    /// Answers whichever session was evicted to make room, so the host can note that its programs
    /// have gone.
    pub fn show(&self, session: SessionId) -> Option<SessionId> {
        let mut evicted = None;
        self.inner.held.update(|held| {
            held.retain(|id| *id != session);
            held.push(session);
            // The oldest goes, unless it is the one being shown, which cannot happen: it was just
            // moved to the end.
            if held.len() > HELD_PER_CLIENT {
                evicted = Some(held.remove(0));
            }
        });
        if self.inner.showing.get_untracked() != Some(session) {
            self.inner.showing.set(Some(session));
        }
        evicted
    }

    /// Takes `session` out of this window, unmounting its subtree.
    pub fn drop_session(&self, session: SessionId) {
        self.inner
            .held
            .update(|held| held.retain(|id| *id != session));
        if self.inner.showing.get_untracked() == Some(session) {
            let next = self.inner.held.with_untracked(|held| held.last().copied());
            self.inner.showing.set(next);
        }
    }

    /// Brings this window to the front.
    pub fn focus(&self) {
        if let Some(handle) = self.inner.handle.as_ref() {
            handle.focus();
        }
    }

    /// Says what this window is called.
    pub fn set_title(&self, title: &str) {
        if let Some(handle) = self.inner.handle.as_ref() {
            handle.set_title(title);
        }
    }
}

/// Publishes `client` to the subtrees this window draws.
pub fn provide(client: Client) {
    zgui::reactive::provide_local_context(client);
}

/// The window this component is in.
///
/// # Panics
///
/// If none was provided above it. That is a wiring mistake, and nothing can carry on from it.
#[must_use]
pub fn use_client() -> Client {
    zgui::reactive::use_local_context::<Client>().expect("a client is provided by the window")
}

#[cfg(test)]
mod tests {
    use super::{Client, ClientId, HELD_PER_CLIENT};
    use crate::session::SessionId;
    use slotmap::SlotMap;

    fn ready() {
        zgui::reactive::install().expect("the reactive runtime installs");
    }

    /// Some session names, which is all these tests need of a session.
    fn names(count: usize) -> Vec<SessionId> {
        let mut map: SlotMap<SessionId, ()> = SlotMap::with_key();
        (0..count).map(|_| map.insert(())).collect()
    }

    #[test]
    fn showing_a_session_mounts_it() {
        ready();
        let client = Client::new(ClientId::default(), None);
        let ids = names(1);
        assert_eq!(client.show(ids[0]), None);
        assert!(client.holds(ids[0]));
        assert_eq!(client.showing_untracked(), Some(ids[0]));
    }

    #[test]
    fn going_back_to_a_held_session_evicts_nothing() {
        ready();
        let client = Client::new(ClientId::default(), None);
        let ids = names(2);
        client.show(ids[0]);
        client.show(ids[1]);
        // The point of holding: the terminals and the scroll of the first are still mounted.
        assert_eq!(client.show(ids[0]), None);
        assert!(client.holds(ids[1]));
    }

    #[test]
    fn the_oldest_session_is_the_one_evicted() {
        ready();
        let client = Client::new(ClientId::default(), None);
        let ids = names(HELD_PER_CLIENT + 1);
        for id in &ids[..HELD_PER_CLIENT] {
            assert_eq!(client.show(*id), None);
        }
        assert_eq!(client.show(ids[HELD_PER_CLIENT]), Some(ids[0]));
        assert!(!client.holds(ids[0]));
    }

    #[test]
    fn dropping_what_is_showing_falls_back_to_the_one_before_it() {
        ready();
        let client = Client::new(ClientId::default(), None);
        let ids = names(2);
        client.show(ids[0]);
        client.show(ids[1]);
        client.drop_session(ids[1]);
        assert_eq!(client.showing_untracked(), Some(ids[0]));
    }

    #[test]
    fn dropping_the_last_session_shows_nothing() {
        ready();
        let client = Client::new(ClientId::default(), None);
        let ids = names(1);
        client.show(ids[0]);
        client.drop_session(ids[0]);
        assert_eq!(client.showing_untracked(), None);
    }
}
