//! The suggestion popup.
//!
//! No language server is started: what is asserted is the layer between one and the popup — how a
//! list is ranked as somebody types, what accepting one puts into the buffer, and that an answer
//! for a prefix nobody is typing any more is dropped rather than drawn.
//!
//! The parts that need a server — the request itself, resolving documentation — are the client's,
//! and are asserted in `zdt-lsp`.

use std::cell::RefCell;
use std::rc::Rc;

use zdt::completion::{Completion, Item, literal, prefix_at, rank, replacement};
use zdt::settings::Settings;
use zgui::prelude::*;
use zgui::view;
use zgui_editor::{EditorHandle, EditorProps};
use zgui_testkit_view::Window;

#[allow(unused_imports)]
use zgui_editor::Editor;

/// Runs `body` inside a reactive scope.
fn in_scope<R>(body: impl FnOnce() -> R) -> R {
    let window = Window::open();
    window.scope.with(body)
}

/// An editor holding `text`, with the caret at the end of it.
///
/// A real one rather than a stub: what `prefix_at` and `replacement` do is read a rope through a
/// handle, and a stub that answered differently would be testing itself.
fn editor(text: &str) -> (Window, EditorHandle) {
    let window = Window::open();
    window.place(window.root, 0.0, 0.0, 800.0, 600.0);

    let taken: Rc<RefCell<Option<EditorHandle>>> = Rc::new(RefCell::new(None));
    let text = text.to_owned();
    let built = {
        let taken = Rc::clone(&taken);
        window.scope.with(|| {
            let view = view! {
                Editor(
                    text = text.clone(),
                    autofocus = false,
                    on_ready = Box::new(move |handle| {
                        *taken.borrow_mut() = Some(handle);
                    }) as Box<dyn Fn(EditorHandle)>,
                )
            };
            let mut built = view.into_view().build(&mut window.cx.cx());
            built.mount(&window.dom_handle, window.root, None);
            built
        })
    };
    // The view outlives the test; dropping it would unmount the editor.
    std::mem::forget(built);
    window.frame();

    let handle = taken.borrow_mut().take().expect("on_ready ran at build");
    // The caret goes where somebody typing would have left it.
    let end = handle.query(|snapshot| snapshot.rope().len_bytes());
    handle.command(zgui_editor::Command::SetSelections {
        selections: vec![zgui_editor::Selection::caret(end)],
        primary: 0,
    });
    window.frame();
    (window, handle)
}

/// One suggestion from a server, with only the fields that matter here.
fn item(label: &str) -> lsp_types::CompletionItem {
    lsp_types::CompletionItem {
        label: label.to_owned(),
        ..Default::default()
    }
}

/// The labels of a ranked list, for the assertions that are about order.
fn labels(items: &[Item]) -> Vec<&str> {
    items.iter().map(|one| one.label.as_str()).collect()
}

#[test]
fn a_popup_that_has_not_been_opened_is_not_open() {
    in_scope(|| {
        let completion = Completion::new(Settings::new(zdt_core::Config::default(), None), None);
        assert!(!completion.is_open());
        assert!(completion.items().is_empty());
        assert!(completion.docs().is_none());
    });
}

#[test]
fn closing_it_forgets_everything() {
    // Including the generation, so that an answer already on its way does not reopen a popup
    // somebody has just dismissed.
    in_scope(|| {
        let completion = Completion::new(Settings::new(zdt_core::Config::default(), None), None);
        completion.close();
        assert!(!completion.is_open());
        assert_eq!(completion.at(), 0);
    });
}

#[test]
fn stepping_through_an_empty_list_does_nothing() {
    in_scope(|| {
        let completion = Completion::new(Settings::new(zdt_core::Config::default(), None), None);
        completion.step(1);
        completion.step(-1);
        assert_eq!(completion.at(), 0);
    });
}

#[test]
fn what_the_server_sent_first_is_shown_first() {
    // rust-analyzer puts the most likely completion at the top, and re-sorting it alphabetically
    // throws away the one thing the server knows that this does not.
    let items = [item("zebra"), item("apple"), item("mango")];
    assert_eq!(labels(&rank(&items, "")), ["zebra", "apple", "mango"]);
}

#[test]
fn typing_narrows_the_list_without_asking_again() {
    // The whole reason typing inside a word costs nothing: the list is re-ranked in memory.
    let items = [item("push"), item("pop"), item("len"), item("push_str")];

    assert_eq!(rank(&items, "").len(), 4);
    assert_eq!(rank(&items, "pu").len(), 2, "only what matches");
    assert!(rank(&items, "zzz").is_empty());
}

#[test]
fn the_word_behind_the_caret_is_what_is_being_completed() {
    let (_window, handle) = editor("let value = thing.push");
    let (word, range) = prefix_at(&handle).expect("the caret is in a word");
    assert_eq!(word, "push");
    assert_eq!(range.len(), 4);
}

#[test]
fn a_caret_that_is_not_in_a_word_is_completing_nothing() {
    // A dot is a trigger character rather than a prefix: what follows it is a fresh question.
    let (_window, handle) = editor("let value = thing.");
    assert!(prefix_at(&handle).is_none());

    let (_empty_window, empty) = editor("");
    assert!(prefix_at(&empty).is_none());
}

#[test]
fn a_number_being_typed_is_not_an_identifier() {
    // Otherwise typing `let x = 12` would ask the server what `12` could be.
    let (_window, handle) = editor("let x = 12");
    assert!(prefix_at(&handle).is_none());
}

#[test]
fn accepting_replaces_the_whole_prefix() {
    // The defect this prevents is the classic one: completing `pu` to `push` and getting `pupush`.
    let (_window, handle) = editor("thing.pu");
    let (word, range) = prefix_at(&handle).expect("the caret is in a word");
    assert_eq!(word, "pu");

    let (over, text) = replacement(
        &item("push"),
        range.clone(),
        &handle,
        zdt_lsp::Encoding::Utf8,
    );
    assert_eq!(over, range, "the whole of what was typed is replaced");
    assert_eq!(text, "push");
}

#[test]
fn a_server_that_named_its_own_range_gets_it() {
    // `rust-analyzer` completing a method replaces the dot as well as the word, and a client that
    // ignored the range would leave the dot behind.
    let (_window, handle) = editor("thing.pu");
    let mut offered = item("push");
    offered.text_edit = Some(lsp_types::CompletionTextEdit::Edit(lsp_types::TextEdit {
        range: lsp_types::Range::new(
            lsp_types::Position::new(0, 5),
            lsp_types::Position::new(0, 8),
        ),
        new_text: ".push".to_owned(),
    }));

    let (over, text) = replacement(&offered, 6..8, &handle, zdt_lsp::Encoding::Utf8);
    assert_eq!(over, 5..8, "the server's range, not the prefix");
    assert_eq!(text, ".push");
}

#[test]
fn a_plain_completion_keeps_its_dollar_signs() {
    // A label like `cost $5` is an ordinary string in half the languages there are, and an editor
    // that read it as a snippet would put `cost ` into the file.
    let (_window, handle) = editor("x");
    let mut plain = item("cost");
    plain.insert_text = Some("cost $5".to_owned());
    plain.insert_text_format = Some(lsp_types::InsertTextFormat::PLAIN_TEXT);

    let (_, text) = replacement(&plain, 0..1, &handle, zdt_lsp::Encoding::Utf8);
    assert_eq!(text, "cost $5");
}

#[test]
fn a_snippet_goes_in_as_what_it_would_have_looked_like() {
    // The client says it does not do snippets. Servers send them anyway, and `foo(${1:x})` typed
    // into a file as its own source is worse than no completion at all.
    let (_window, handle) = editor("x");
    let mut snippet = item("foo");
    snippet.insert_text = Some("foo(${1:name})".to_owned());
    snippet.insert_text_format = Some(lsp_types::InsertTextFormat::SNIPPET);

    let (_, text) = replacement(&snippet, 0..1, &handle, zdt_lsp::Encoding::Utf8);
    assert_eq!(text, "foo(name)");
    assert_eq!(literal("$0"), "");
}

#[test]
fn a_kind_becomes_one_of_four_groups() {
    // Twenty-five distinct tones would be twenty-five nobody can tell apart at twelve pixels.
    let of = |kind| Item {
        label: "x".to_owned(),
        kind: Some(kind),
        detail: None,
        index: 0,
    };
    use lsp_types::CompletionItemKind as Kind;

    assert_eq!(of(Kind::FUNCTION).tone(), "function");
    assert_eq!(of(Kind::METHOD).tone(), "function");
    assert_eq!(of(Kind::STRUCT).tone(), "type");
    assert_eq!(of(Kind::FIELD).tone(), "value");
    assert_eq!(of(Kind::KEYWORD).tone(), "keyword");
    assert_eq!(of(Kind::TEXT).tone(), "text");
}
