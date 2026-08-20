//! What a session is, and what it outlives.
//!
//! The rule the whole tier is built around: a session belongs to the application and not to a
//! window. Getting it wrong is silent — a signal made under a window reads fine until that window
//! is disposed of, and then answers a default. So the lifetime is pinned here.

use std::path::PathBuf;

use zdt::session::SessionKey;
use zdt::session::host::{Revealed, SessionHost};
use zgui_testkit_view::Window;

/// A directory that removes itself.
struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "zdt-sessions-{}-{}-{:?}",
            std::process::id(),
            name,
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("the directory is made");
        Self(std::fs::canonicalize(&path).expect("it canonicalises"))
    }

    fn key(&self) -> SessionKey {
        SessionKey::of(&self.0).expect("it is a directory")
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A registry, made the way `main` makes one.
///
/// The owner is taken *outside* the window, which is the whole invariant: a session made under a
/// window's scope loses everything it holds when that window goes.
fn host(window: &Window) -> SessionHost {
    // Outside the window's scope, so nothing above it can dispose of it.
    let root = zdt::session::host::detached_root();
    window.scope.with(|| {
        let global = zdt::app::global::install();
        let host = SessionHost::new(global, root);
        zdt::session::host::provide(host.clone());
        host
    })
}

#[test]
fn one_directory_is_one_session() {
    let window = Window::open();
    let place = Temp::new("same");
    let host = host(&window);

    let first = host.open(place.key());
    let second = host.open(place.key());
    assert_eq!(first, second, "asking twice answers the same session");
    assert_eq!(host.list_untracked().len(), 1);
}

#[test]
fn a_subdirectory_is_a_session_of_its_own() {
    let window = Window::open();
    let outer = Temp::new("outer");
    std::fs::create_dir_all(outer.0.join("inner")).expect("the directory is made");
    let inner = SessionKey::of(&outer.0.join("inner")).expect("it is a directory");
    let host = host(&window);

    assert_ne!(host.open(outer.key()), host.open(inner));
    assert_eq!(host.list_untracked().len(), 2);
}

#[test]
fn a_session_keeps_its_workspace_after_the_window_that_attached_it_goes() {
    // The whole reason a session hangs off the application's scope. A workspace built under a
    // window would lose every buffer the moment somebody closed that window.
    let window = Window::open();
    let place = Temp::new("outlives");
    let host = host(&window);
    let session = host.session(host.open(place.key())).expect("it was made");

    // A buffer and a split, both made while the window is up. Opening a file goes through a
    // worker, so the document is put in directly: what is under test is the lifetime, and not the
    // reading.
    let file = place.0.join("kept.txt");
    let (buffer, split) = window.scope.with(|| {
        let buffer = session
            .workspace()
            .open_document(Some(file.clone()), zgui_editor::Document::new("something"));
        let split = session.workspace().split(zdt::workspace::Axis::Vertical);
        (buffer, split)
    });
    assert!(split.is_some(), "the split was made");
    let before = session.workspace().order().len();

    // The window goes. The session must not.
    drop(window);

    assert_eq!(session.workspace().order().len(), before);
    assert_eq!(
        session.workspace().find_path(&file),
        Some(buffer),
        "the buffer is still there",
    );
    assert_eq!(
        session.workspace().windows().len(),
        2,
        "and so is the split"
    );
}

#[test]
fn a_session_is_rooted_at_the_directory_it_was_opened_on() {
    let window = Window::open();
    let place = Temp::new("rooted");
    let host = host(&window);
    let session = host.session(host.open(place.key())).expect("it was made");

    assert_eq!(session.project().root(), place.0);
    assert_eq!(
        session.name(),
        place.0.file_name().unwrap().to_string_lossy()
    );
}

#[test]
fn revealing_a_directory_that_is_open_does_not_make_a_second_session() {
    let window = Window::open();
    let place = Temp::new("reveal");
    let host = host(&window);

    assert_eq!(host.reveal(place.key(), &[]), Revealed::Opened);
    // No window is registered, so there is nowhere to show it; it is still the same session.
    assert_eq!(host.reveal(place.key(), &[]), Revealed::Shown);
    assert_eq!(host.list_untracked().len(), 1);
}

#[test]
fn the_last_session_cannot_be_killed() {
    // The editor is always in a session, so there is no "no session" state to be left in.
    let window = Window::open();
    let one = Temp::new("kill-one");
    let two = Temp::new("kill-two");
    let host = host(&window);

    let first = host.open(one.key());
    let second = host.open(two.key());
    assert!(host.kill(second));
    assert!(!host.kill(first), "the last one stays");
    assert_eq!(host.list_untracked().len(), 1);
}

#[test]
fn a_killed_session_is_gone_from_the_registry() {
    let window = Window::open();
    let one = Temp::new("gone-one");
    let two = Temp::new("gone-two");
    let host = host(&window);

    host.open(one.key());
    let second = host.open(two.key());
    assert!(host.kill(second));
    assert_eq!(host.find(&two.key()), None);
    // And asking again makes a new one rather than answering the dead one.
    assert_ne!(host.open(two.key()), second);
}

#[test]
fn a_client_holds_the_session_it_leaves_so_going_back_is_free() {
    // The point of holding rather than unmounting: the terminals and the scroll of a session
    // somebody switched away from are still there when they switch back.
    let window = Window::open();
    let one = Temp::new("hold-one");
    let two = Temp::new("hold-two");
    let host = host(&window);

    let client = host.register_client(None);
    let first = host.open(one.key());
    let second = host.open(two.key());

    client.show(first);
    client.show(second);
    assert!(client.holds(first), "the one left behind is still mounted");
    assert_eq!(client.showing_untracked(), Some(second));

    client.show(first);
    assert_eq!(client.showing_untracked(), Some(first));
    assert!(client.holds(second));
}

#[test]
fn revealing_a_session_a_window_holds_focuses_that_window() {
    let window = Window::open();
    let one = Temp::new("focus-one");
    let two = Temp::new("focus-two");
    let host = host(&window);

    let client = host.register_client(None);
    let first = host.open(one.key());
    client.show(first);

    // Already on screen there.
    assert_eq!(host.reveal(one.key(), &[]), Revealed::Focused);
    // Not on screen anywhere, so it is put into the window that exists.
    assert_eq!(host.reveal(two.key(), &[]), Revealed::Opened);
    assert_eq!(host.reveal(one.key(), &[]), Revealed::Shown);
}

#[test]
fn a_session_that_no_window_holds_is_still_in_the_registry() {
    // A detached session, in tmux's sense: running, with nobody looking at it.
    let window = Window::open();
    let place = Temp::new("detached");
    let host = host(&window);

    let id = host.open(place.key());
    assert!(host.client_holding(id).is_none());
    assert_eq!(host.list_untracked().len(), 1);
    assert!(!host.session(id).expect("it is there").is_attached());
}
