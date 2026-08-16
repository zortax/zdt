//! Where a provided context can and cannot be reached.
//!
//! This is the rule the whole interface is wired around, and getting it wrong is silent: a lookup
//! that answers `None` inside a task makes the feature that needed it do nothing at all, with no
//! error anywhere. It cost the hover panel, the completion popup and the rename box, each of which
//! looked up what it needed *after* an await and quietly gave up.
//!
//! The rule: **look a context up while building, and capture the handle.** Never look one up
//! inside a timer's callback or after an await.

use std::cell::RefCell;
use std::rc::Rc;

use zgui::prelude::*;
use zgui::view;
use zgui_testkit_view::Window;
use zgui_ui::prelude::{ToastCorner, ToasterProps};

/// Something to look for.
#[derive(Clone, Copy)]
struct Marker;

/// What a probe found, once it has run.
type Found = Rc<RefCell<Option<bool>>>;

#[test]
fn a_component_inside_a_toaster_finds_its_queue() {
    // Which is what lets the announcements be taken once, at the root, and handed to everything
    // that announces anything.
    let window = Window::open();
    window.place(window.root, 0.0, 0.0, 800.0, 600.0);
    let found: Found = Rc::new(RefCell::new(None));

    let built = {
        let found = Rc::clone(&found);
        window.scope.with(|| {
            let view = view! {
                Toaster(corner = ToastCorner::BottomRight) {
                    Probe(found = found.clone())
                }
            };
            let mut built = view.into_view().build(&mut window.cx.cx());
            built.mount(&window.dom_handle, window.root, None);
            built
        })
    };
    std::mem::forget(built);
    window.frame();

    assert_eq!(*found.borrow(), Some(true));
}

#[zgui::component]
fn Probe(found: Found) -> impl IntoView {
    *found.borrow_mut() = Some(zgui_ui::toast::use_toaster().is_some());
    view! { box() }
}

#[test]
fn a_context_is_gone_by_the_time_a_timer_fires() {
    // The defect this documents: every debounce in this application runs its work in a timer
    // callback, and anything that looked up a language server there found nothing and returned.
    let window = Window::open();
    let seen: Found = Rc::new(RefCell::new(None));

    window.scope.with(|| {
        zgui::reactive::provide_local_context(Marker);
        let seen = Rc::clone(&seen);
        if let Some(timers) = zgui::view::time::Timers::current() {
            let handle = timers.set_timeout(std::time::Duration::from_millis(1), move || {
                *seen.borrow_mut() = Some(zgui::reactive::use_local_context::<Marker>().is_some());
            });
            std::mem::forget(handle);
        }
    });
    for _ in 0..10 {
        window.advance(std::time::Duration::from_millis(5));
        window.frame();
    }

    assert_eq!(
        *seen.borrow(),
        Some(false),
        "a timer callback runs outside the scope that started it — capture the handle instead"
    );
}

#[test]
fn a_context_is_gone_after_an_await() {
    // The same, for every request to a language server: the answer arrives in a continuation, and
    // a panel looked up there is a panel that never opens.
    let window = Window::open();
    let seen: Found = Rc::new(RefCell::new(None));

    window.scope.with(|| {
        zgui::reactive::provide_local_context(Marker);
        let seen = Rc::clone(&seen);
        zgui::task::spawn_detached(async move {
            zgui::task::blocking(|| std::thread::sleep(std::time::Duration::from_millis(1))).await;
            *seen.borrow_mut() = Some(zgui::reactive::use_local_context::<Marker>().is_some());
        });
    });
    for _ in 0..60 {
        window.advance(std::time::Duration::from_millis(5));
        window.frame();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    assert_eq!(
        *seen.borrow(),
        Some(false),
        "a continuation runs outside the scope that started it — capture the handle instead"
    );
}

#[test]
fn a_captured_handle_survives_both() {
    // Which is the pattern everything in the interface uses: look it up while building, move the
    // handle into the closure.
    let window = Window::open();
    let seen: Found = Rc::new(RefCell::new(None));

    window.scope.with(|| {
        zgui::reactive::provide_local_context(Marker);
        // Looked up *here*, while there is a scope to look in.
        let held = zgui::reactive::use_local_context::<Marker>();
        let seen = Rc::clone(&seen);
        zgui::task::spawn_detached(async move {
            zgui::task::blocking(|| std::thread::sleep(std::time::Duration::from_millis(1))).await;
            *seen.borrow_mut() = Some(held.is_some());
        });
    });
    for _ in 0..60 {
        window.advance(std::time::Duration::from_millis(5));
        window.frame();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    assert_eq!(*seen.borrow(), Some(true));
}
