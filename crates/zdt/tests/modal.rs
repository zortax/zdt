//! The modal layer joined to a real editor.
//!
//! The grammar itself is asserted in `zdt-vim`, against a rope and nothing else. What is asserted
//! here is the seam: that a key event reaches the engine, that what the engine asks for reaches
//! the editor, and that the two agree about where the caret is.

use std::cell::RefCell;
use std::rc::Rc;

use zdt::vim::Vim;
use zdt::workspace::Workspace;
use zdt_core::Project;
use zgui::prelude::*;
use zgui::vocab::{Key, Modifiers, NamedKey};
use zgui_editor::EditorHandle;
use zgui_testkit_view::Window;

#[allow(unused_imports)]
use zgui_editor::{Editor, EditorProps};

/// An editor with the modal layer in front of it.
struct Modal {
    window: Window,
    node: zgui::view::NodeId,
    handle: EditorHandle,
    vim: Vim,
}

fn mount(text: &str) -> Modal {
    let window = Window::open();
    window.place(window.root, 0.0, 0.0, 800.0, 600.0);

    let taken: Rc<RefCell<Option<EditorHandle>>> = Rc::new(RefCell::new(None));
    let vim: Rc<RefCell<Option<Vim>>> = Rc::new(RefCell::new(None));
    let text = text.to_string();

    let built = {
        let taken = Rc::clone(&taken);
        let kept = Rc::clone(&vim);
        window.scope.with(|| {
            let workspace = Workspace::new(Project::at("/project"));
            let layer = Vim::new(workspace);
            kept.borrow_mut().replace(layer.clone());

            let filter: zgui_editor::KeyFilter = Box::new(
                move |event: &zgui::vocab::KeyEvent,
                      modifiers: Modifiers,
                      handle: &EditorHandle| {
                    match zdt::keys::chord_of(event, modifiers) {
                        Some(chord) => layer.key(chord, handle),
                        None => false,
                    }
                },
            );

            let view = view! {
                Editor(
                    text = text.clone(),
                    autofocus = false,
                    on_ready = Box::new(move |handle| {
                        *taken.borrow_mut() = Some(handle);
                    }) as Box<dyn Fn(EditorHandle)>,
                    on_key = filter,
                )
            };
            use zgui::view::IntoView;
            let mut built = view.into_view().build(&mut window.cx.cx());
            built.mount(&window.dom_handle, window.root, None);
            built
        })
    };
    // The view outlives the test; dropping it would unmount the editor.
    std::mem::forget(built);
    window.frame();

    let handle = taken.borrow_mut().take().expect("on_ready ran at build");
    let vim = vim.borrow_mut().take().expect("the layer was made");
    let node = window
        .dom
        .tree()
        .children(window.root)
        .into_iter()
        .find(|child| !window.dom.tree().is_marker(*child))
        .expect("the editor mounted one element");

    Modal {
        window,
        node,
        handle,
        vim,
    }
}

impl Modal {
    /// Types one key.
    fn press(&self, key: Key) {
        self.window.dispatcher().key(self.node, key);
        self.window.frame();
    }

    /// Types one key with modifiers held.
    fn press_with(&self, key: Key, modifiers: Modifiers) {
        self.window
            .dispatcher()
            .with_modifiers(modifiers)
            .key(self.node, key);
        self.window.frame();
    }

    /// Types a run of plain characters.
    fn keys(&self, text: &str) {
        for character in text.chars() {
            self.press(Key::character(character.to_string()));
        }
    }

    fn text(&self) -> String {
        self.handle.query(|snapshot| snapshot.rope().to_string())
    }

    fn cursor(&self) -> usize {
        self.handle
            .query(|snapshot| snapshot.selections().primary().head)
    }
}

#[test]
fn a_letter_is_a_command_rather_than_text() {
    // The whole point of the modal layer: `l` moves rather than inserting an `l`.
    let modal = mount("hello");
    modal.keys("l");
    assert_eq!(modal.text(), "hello");
    assert_eq!(modal.cursor(), 1);
}

#[test]
fn a_motion_and_an_operator_reach_the_editor() {
    let modal = mount("hello world");
    modal.keys("dw");
    assert_eq!(modal.text(), "world");
    assert_eq!(modal.cursor(), 0);
}

#[test]
fn insert_mode_gives_the_keys_back_to_the_editor() {
    // Typing is the editor's own business — its auto-indent and its undo grouping are better than
    // anything the engine could do with the keys.
    let modal = mount("world");
    modal.keys("i");
    assert_eq!(modal.vim.mode(), zdt_vim::Mode::Insert);
    modal.keys("hello ");
    assert_eq!(modal.text(), "hello world");
    modal.press(Key::Named(NamedKey::Escape));
    assert_eq!(modal.vim.mode(), zdt_vim::Mode::Normal);
}

#[test]
fn escape_puts_the_caret_back_onto_a_character() {
    let modal = mount("ab");
    modal.keys("iXY");
    assert_eq!(modal.text(), "XYab");
    modal.press(Key::Named(NamedKey::Escape));
    assert_eq!(modal.cursor(), 1, "on the `Y`, where vim leaves it");
}

#[test]
fn the_editors_own_undo_takes_back_what_the_engine_did() {
    let modal = mount("hello world");
    modal.keys("dw");
    assert_eq!(modal.text(), "world");
    modal.keys("u");
    assert_eq!(modal.text(), "hello world");
}

#[test]
fn a_control_chord_reaches_the_keymap() {
    let modal = mount("hello world");
    modal.keys("dw");
    modal.keys("u");
    modal.press_with(Key::character("r"), Modifiers::CONTROL);
    assert_eq!(modal.text(), "world", "control-r redid it");
}

#[test]
fn the_leader_is_the_space_bar() {
    // A leader that arrived as a bare character rather than the named key would never match.
    let modal = mount("hello");
    modal.press(Key::Named(NamedKey::Space));
    assert_eq!(modal.vim.pending(), "<Space>", "it is waiting for the rest");
    assert_eq!(modal.text(), "hello", "and it did not type a space");
}

#[test]
fn a_pending_sequence_is_echoed_and_then_cleared() {
    let modal = mount("hello");
    modal.keys("g");
    assert_eq!(modal.vim.pending(), "g");
    modal.keys("g");
    assert_eq!(modal.vim.pending(), "");
}

#[test]
fn a_count_is_echoed_while_it_is_being_typed() {
    let modal = mount("one two three four");
    modal.keys("2");
    assert_eq!(modal.vim.pending(), "2");
    modal.keys("dw");
    assert_eq!(modal.text(), "three four");
    assert_eq!(modal.vim.pending(), "");
}

#[test]
fn visual_mode_selects_in_the_editor() {
    let modal = mount("hello world");
    modal.keys("vll");
    assert_eq!(modal.vim.mode(), zdt_vim::Mode::Visual);
    let selection = modal
        .handle
        .query(|snapshot| snapshot.selections().primary());
    assert_eq!(selection.anchor, 0);
    assert_eq!(selection.head, 2);
    modal.keys("d");
    assert_eq!(modal.text(), "lo world");
}

#[test]
fn several_carets_survive_a_block_insert() {
    let modal = mount("one\ntwo\nthree");
    modal.press_with(Key::character("v"), Modifiers::CONTROL);
    modal.keys("jj");
    modal.keys("I");
    let count = modal.handle.query(|snapshot| snapshot.selections().len());
    assert_eq!(count, 3, "one caret per line of the block");
    modal.keys("- ");
    assert_eq!(modal.text(), "- one\n- two\n- three");
}

#[test]
fn the_mode_the_layer_reports_is_the_mode_the_editor_is_drawn_in() {
    // The status line reads the first and the caret is drawn by the second; if they disagree the
    // editor lies about what it will do with the next key.
    let modal = mount("hello");
    assert_eq!(modal.vim.mode(), zdt_vim::Mode::Normal);
    modal.keys("v");
    assert_eq!(modal.vim.mode(), zdt_vim::Mode::Visual);
    modal.press(Key::Named(NamedKey::Escape));
    assert_eq!(modal.vim.mode(), zdt_vim::Mode::Normal);
    modal.keys("i");
    assert_eq!(modal.vim.mode(), zdt_vim::Mode::Insert);
}

#[test]
fn which_key_offers_what_could_come_next() {
    let modal = mount("hello");
    assert!(
        modal.vim.continuations().is_empty(),
        "nothing is pending, so there is nothing to offer"
    );

    modal.press(Key::Named(NamedKey::Space));
    let next = modal.vim.continuations();
    assert!(next.len() > 10, "the leader map is not that small");

    let find = next
        .iter()
        .find(|one| one.keys == "f")
        .expect("`f` is in the leader map");
    assert_eq!(find.label, "Find");
    assert!(!find.runs, "it is a group rather than a binding");

    let save = next
        .iter()
        .find(|one| one.keys == "w")
        .expect("`w` is in the leader map");
    assert_eq!(save.label, "Save");
    assert!(save.runs);
}

#[test]
fn which_key_follows_the_sequence_into_a_group() {
    // The rows are keyed by what they say as well as by their key, because a row reused from the
    // group above would keep the label it was built with — and the two groups share plenty of
    // keys.
    let modal = mount("hello");
    modal.press(Key::Named(NamedKey::Space));
    modal.keys("f");

    let next = modal.vim.continuations();
    assert!(
        next.iter()
            .all(|one| one.label.starts_with("Find") || one.label.starts_with("Resume")),
        "every row belongs to the Find group: {next:?}"
    );
    assert!(
        next.iter()
            .any(|one| one.keys == "f" && one.label == "Find files")
    );
}

#[test]
fn which_key_has_nothing_to_offer_once_a_sequence_resolves() {
    let modal = mount("hello world");
    modal.keys("d");
    assert!(
        !modal.vim.continuations().is_empty(),
        "`d` waits for a motion"
    );
    modal.keys("w");
    assert!(modal.vim.continuations().is_empty());
    assert_eq!(modal.text(), "world");
}
