//! The documentation panel, opened and closed and opened again.
//!
//! Twice matters: anything held across a rebuild — an observation, a mounted editor for a fenced
//! block, a context looked up in the wrong scope — shows up on the second open and never the
//! first.

use std::cell::RefCell;
use std::rc::Rc;

use zdt::ui::hover::{Hover, HoverPanelProps};
use zgui::prelude::*;
use zgui::view;
use zgui_testkit_view::Window;

/// A window with the panel mounted and the hover state provided.
fn mounted() -> (Window, Hover) {
    let window = Window::open();
    window.place(window.root, 0.0, 0.0, 800.0, 600.0);

    let taken: Rc<RefCell<Option<Hover>>> = Rc::new(RefCell::new(None));
    let built = {
        let taken = Rc::clone(&taken);
        window.scope.with(|| {
            let hover = Hover::new();
            zdt::ui::hover::provide(hover);
            *taken.borrow_mut() = Some(hover);

            let view = view! { HoverPanel() };
            let mut built = view.into_view().build(&mut window.cx.cx());
            built.mount(&window.dom_handle, window.root, None);
            built
        })
    };
    std::mem::forget(built);
    window.frame();

    let hover = taken.borrow_mut().take().expect("the hover was made");
    (window, hover)
}

/// Somewhere for the panel to be anchored.
fn caret() -> zgui_editor::CaretRect {
    zgui_editor::CaretRect {
        x: 100.0,
        y: 200.0,
        width: 2.0,
        height: 16.0,
    }
}

/// Lets the presence and its animations run.
fn settle(window: &Window) {
    for _ in 0..40 {
        window.advance(std::time::Duration::from_millis(10));
        window.frame();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn plain_prose_opens_twice() {
    let (window, hover) = mounted();

    hover.show_markdown("The number of bytes.", caret());
    settle(&window);
    assert!(hover.is_showing());

    hover.hide();
    settle(&window);
    assert!(!hover.is_showing());

    hover.show_markdown("The number of bytes.", caret());
    settle(&window);
    assert!(hover.is_showing(), "and again");
}

#[test]
fn a_fenced_block_opens_twice() {
    // A fence mounts an editor of its own, which is the one thing in this panel with a worker and
    // a parser behind it — so it is the thing most likely to object to being built again.
    let (window, hover) = mounted();
    let doc = "```rust\npub fn len(&self) -> usize\n```\n\nThe number of bytes.";

    hover.show_markdown(doc, caret());
    settle(&window);
    assert!(hover.is_showing());

    hover.hide();
    settle(&window);

    hover.show_markdown(doc, caret());
    settle(&window);
    assert!(hover.is_showing(), "and again");
}

#[test]
fn opening_over_an_open_one_replaces_it() {
    // Which is what `K` on a second symbol does without the panel closing in between.
    let (window, hover) = mounted();

    hover.show_markdown("first", caret());
    settle(&window);
    hover.show_markdown("```rust\nfn second()\n```", caret());
    settle(&window);

    assert!(hover.is_showing());
}

#[test]
fn it_survives_being_opened_many_times() {
    // The panel is opened and dismissed all day; whatever it holds must not accumulate.
    let (window, hover) = mounted();

    for round in 0..8 {
        hover.show_markdown(
            &format!("```rust\nfn round_{round}()\n```\n\nprose"),
            caret(),
        );
        settle(&window);
        hover.hide();
        settle(&window);
    }
    assert!(!hover.is_showing());
}

/// One frame, and no more: enough to start an exit and not enough to finish one.
fn tick(window: &Window) {
    window.advance(std::time::Duration::from_millis(1));
    window.frame();
}

#[test]
fn opening_it_again_while_it_is_still_leaving() {
    // What the second `K` actually does. The key first *hides* the panel — that is the rule that
    // makes a documentation panel go away on the next keystroke — and the action it then runs asks
    // the server again and shows a new one a few milliseconds later. So the new panel arrives while
    // the old one is still playing its exit, and for a moment the presence holds both.
    let (window, hover) = mounted();

    hover.show_markdown(
        "```rust\npub fn len(&self) -> usize\n```\n\nThe number of bytes.",
        caret(),
    );
    settle(&window);

    hover.hide();
    tick(&window);
    hover.show_markdown(
        "```rust\npub fn is_empty(&self) -> bool\n```\n\nWhether it is empty.",
        caret(),
    );
    settle(&window);

    assert!(hover.is_showing(), "the second one is up");
}

#[test]
fn hiding_and_showing_within_one_frame() {
    // The same, tighter: no frame at all between the two, which is what happens when the answer
    // was already cached and comes back on the same tick.
    let (window, hover) = mounted();

    hover.show_markdown("first", caret());
    settle(&window);

    hover.hide();
    hover.show_markdown("second", caret());
    settle(&window);

    assert!(hover.is_showing());
}

#[test]
fn a_burst_of_opens_and_closes_survives() {
    let (window, hover) = mounted();

    for round in 0..12 {
        hover.hide();
        tick(&window);
        hover.show_markdown(&format!("```rust\nfn round_{round}()\n```"), caret());
        tick(&window);
    }
    settle(&window);
    assert!(hover.is_showing());
}

// ---- The second press ---------------------------------------------------------------------

/// A vim layer with the shipped keymap and every shipped overlay, as the window has.
fn keys() -> (Window, zdt::vim::Vim) {
    let window = Window::open();
    let vim = window.scope.with(|| {
        let workspace = zdt::workspace::Workspace::new(zdt_core::Project::at("/project"));
        let settings = zdt::settings::Settings::new(zdt_core::Config::default(), None);
        let vim = zdt::vim::Vim::new(workspace, settings);
        for (region, shipped, _) in zdt::assets::OVERLAYS {
            vim.load_overlay(region, shipped, None)
                .unwrap_or_else(|problems| panic!("{region} did not read: {problems:?}"));
        }
        vim
    });
    (window, vim)
}

/// `K`, as the keymap spells it.
fn shift_k() -> zdt_vim::chord::Chord {
    zdt_vim::chord::Chord::char('K')
}

#[test]
fn the_key_that_opens_documentation_is_the_key_that_focuses_it() {
    // The panel's whole two-state arrangement rests on this, and it was unreachable: the branch
    // that closes the panel on the next keystroke ran first and closed it, so the action behind
    // `K` then found nothing showing and asked the server all over again. Pressing `K` twice
    // re-fetched the same documentation instead of letting it be scrolled.
    let (_window, vim) = keys();
    assert!(
        vim.chord_runs(shift_k(), "lsp.hover"),
        "`K` is what asks for documentation"
    );
}

#[test]
fn an_unrelated_key_is_not_mistaken_for_it() {
    // The guard has to be about *this* key rather than about any key arriving while the panel is
    // up, or the panel would take the keyboard from the first thing typed after it opened.
    let (_window, vim) = keys();
    assert!(!vim.chord_runs(zdt_vim::chord::Chord::char('j'), "lsp.hover"));
    assert!(!vim.chord_runs(zdt_vim::chord::Chord::char('w'), "lsp.hover"));
}

#[test]
fn focusing_needs_something_to_read() {
    // `focus` answering false is what lets the branch fall through to closing: a panel with
    // nothing in it must not swallow the key.
    let hover = Hover::new();
    assert!(!hover.focus(), "nothing showing, nothing to focus");

    hover.show_markdown("The number of bytes.", caret());
    assert!(hover.focus(), "and now there is");
    assert!(hover.is_focused());
}
