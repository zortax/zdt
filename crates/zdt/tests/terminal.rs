//! The terminals, driven without a window.
//!
//! The emulator itself is `zgui-terminal`'s and is tested there. What is asserted here is the
//! layer above it: a program is started once and handed over once, a float stays off the buffer
//! line, toggling one reaches the same one each time, and terminal mode is a thing the interface
//! can be in and get out of.
//!
//! These start real processes. `true` and `cat` stand in for a shell, because both exist
//! everywhere and neither reads a configuration file.

use zdt::settings::Settings;
use zdt::terminals::{Program, TerminalMode, Terminals};
use zdt::vim::Vim;
use zdt::workspace::Workspace;
use zdt_core::{Config, Project};
use zgui::vocab::{Key, KeyEvent, Modifiers, NamedKey, PhysicalKey};
use zgui_testkit_view::Window;

/// A terminals layer over a scratch workspace.
fn mount(window: &Window) -> (Terminals, Workspace) {
    window.scope.with(|| {
        let workspace = Workspace::new(Project::at("/tmp"));
        let settings = Settings::new(Config::default(), None);
        zdt::workspace::provide(workspace.clone());
        let terminals = Terminals::new(workspace.clone(), settings);
        zdt::terminals::provide(terminals.clone());
        (terminals, workspace)
    })
}

/// A modal layer with the keymap the editor ships with.
fn modal(window: &Window, workspace: &Workspace) -> Vim {
    window.scope.with(|| {
        let keymaps = zdt::keymaps::Keymaps::new();
        keymaps.reset();
        for (name, text, _) in zdt::assets::OVERLAYS {
            keymaps
                .load_overlay(name, text, None)
                .expect("the keymap it ships with reads");
        }
        Vim::new(
            workspace.clone(),
            Settings::new(Config::default(), None),
            keymaps,
        )
    })
}

/// A program that sits there until it is shut down.
fn waiting() -> Program {
    Program::command("cat")
}

#[test]
fn a_terminal_is_a_buffer_on_the_buffer_line() {
    let window = Window::open();
    let (terminals, workspace) = mount(&window);

    let id = window
        .scope
        .with(|| terminals.open(&waiting()))
        .expect("cat starts");

    assert!(workspace.order().contains(&id));
    let buffer = workspace.buffer_untracked(id).expect("a buffer");
    assert!(buffer.is_terminal());
    assert_eq!(buffer.name(), "cat", "named after the program");

    terminals.close(id);
}

#[test]
fn the_program_outlives_the_view_that_draws_it() {
    // The whole reason a terminal is held here rather than by its view: a session taken off
    // screen, or a window somebody closed, must not be a shell that was killed.
    let window = Window::open();
    let (terminals, _) = mount(&window);

    let id = window
        .scope
        .with(|| terminals.open(&waiting()))
        .expect("cat starts");

    let first = terminals.running(id).expect("a view can draw it");
    let second = terminals.running(id).expect("and so can the next one");
    assert!(!first.is_drawn(), "nothing is drawing it in this harness");
    // Two handles onto one program, and never two programs.
    drop(first);
    assert!(terminals.running(id).is_some(), "it is still running");
    drop(second);

    terminals.close(id);
    assert!(
        terminals.running(id).is_none(),
        "closing it is what stops it",
    );
}

#[test]
fn a_float_is_not_on_the_buffer_line() {
    let window = Window::open();
    let (terminals, workspace) = mount(&window);

    window
        .scope
        .with(|| terminals.toggle_float("default", &waiting()));

    let id = terminals.showing().expect("it is showing");
    assert!(
        !workspace.order().contains(&id),
        "a float is reachable by its key and by nothing else"
    );

    terminals.close(id);
}

#[test]
fn toggling_a_float_is_the_same_one_each_time() {
    let window = Window::open();
    let (terminals, _) = mount(&window);

    window
        .scope
        .with(|| terminals.toggle_float("default", &waiting()));
    let first = terminals.showing().expect("it is showing");

    window
        .scope
        .with(|| terminals.toggle_float("default", &waiting()));
    assert_eq!(terminals.showing(), None, "the same key puts it away");

    window
        .scope
        .with(|| terminals.toggle_float("default", &waiting()));
    assert_eq!(
        terminals.showing(),
        Some(first),
        "and brings back the one that was running, not another"
    );
    assert_eq!(terminals.floats().len(), 1);

    terminals.close(first);
}

#[test]
fn floats_are_told_apart_by_name() {
    let window = Window::open();
    let (terminals, _) = mount(&window);

    window.scope.with(|| {
        terminals.toggle_float("default", &waiting());
        terminals.toggle_float("other", &waiting());
    });

    let names: Vec<String> = terminals
        .floats()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(names, vec!["default".to_owned(), "other".to_owned()]);
    assert_eq!(
        terminals.floats().len(),
        2,
        "asking for a second name starts a second program"
    );

    for (_, id) in terminals.floats() {
        terminals.close(id);
    }
}

#[test]
fn showing_a_float_gives_it_the_keys() {
    let window = Window::open();
    let (terminals, workspace) = mount(&window);

    window
        .scope
        .with(|| terminals.toggle_float("default", &waiting()));
    let id = terminals.showing().expect("it is showing");
    assert_eq!(
        terminals.mode_of(id),
        TerminalMode::Terminal,
        "and the keys go to the program in it"
    );

    let vim = modal(&window, &workspace);
    window.scope.with(|| terminals.enter_normal_mode(&vim, id));
    assert_eq!(
        terminals.mode_of(id),
        TerminalMode::Normal,
        "`<C-\\><C-n>` takes them back"
    );
    assert_eq!(
        terminals.showing(),
        Some(id),
        "and leaves the float where it is, so its history can be read"
    );

    terminals.close(id);
}

#[test]
fn a_terminal_starts_with_the_program_reading() {
    let window = Window::open();
    let (terminals, _) = mount(&window);

    let id = window
        .scope
        .with(|| terminals.open(&waiting()))
        .expect("cat starts");
    assert_eq!(terminals.mode_of(id), TerminalMode::Terminal);

    terminals.close(id);
}

#[test]
fn which_mode_a_terminal_is_in_is_a_fact_about_that_terminal() {
    let window = Window::open();
    let (terminals, workspace) = mount(&window);
    let vim = modal(&window, &workspace);

    let one = window
        .scope
        .with(|| terminals.open(&waiting()))
        .expect("cat starts");
    let two = window
        .scope
        .with(|| terminals.open(&waiting()))
        .expect("cat starts");

    window.scope.with(|| terminals.enter_normal_mode(&vim, one));
    assert_eq!(terminals.mode_of(one), TerminalMode::Normal);
    assert_eq!(
        terminals.mode_of(two),
        TerminalMode::Terminal,
        "leaving one terminal leaves the others alone"
    );

    terminals.close(one);
    terminals.close(two);
}

#[test]
fn closing_one_forgets_everything_about_it() {
    let window = Window::open();
    let (terminals, _) = mount(&window);

    window
        .scope
        .with(|| terminals.toggle_float("lazygit", &waiting()));
    let id = terminals.showing().expect("it is showing");

    terminals.close(id);
    assert!(terminals.floats().is_empty());
    assert_eq!(terminals.showing(), None);
    assert_eq!(terminals.mode_of(id), TerminalMode::Terminal);
    assert!(terminals.running(id).is_none());
}

#[test]
fn a_program_that_will_not_start_says_so() {
    let window = Window::open();
    let (terminals, workspace) = mount(&window);

    let started = window.scope.with(|| {
        terminals.open(&Program::command(
            "a-program-that-is-not-installed-anywhere-at-all",
        ))
    });

    assert!(started.is_none());
    let message = workspace.message().expect("it complains");
    assert!(message.error);
    assert!(message.text.contains("cannot start"));
}

#[test]
fn a_program_is_named_by_its_last_path_component() {
    assert_eq!(Program::command("/usr/bin/btm").name(), "btm");
    assert_eq!(Program::command("lazygit --help").name(), "lazygit");
    assert_eq!(Program::shell().name(), "shell");
}

// ---- The keys ------------------------------------------------------------------------------

/// A press of a character key, as the platform reports one.
fn typed(character: &str) -> KeyEvent {
    KeyEvent {
        key: Key::character(character),
        key_without_modifiers: Key::character(character),
        physical: PhysicalKey::Unidentified(0),
        location: zgui::vocab::KeyLocation::Standard,
        repeat: false,
    }
}

/// One key for the program, answering whether the keymap took it.
fn press(
    terminals: &Terminals,
    vim: &Vim,
    id: zdt::workspace::BufferId,
    event: &KeyEvent,
    modifiers: Modifiers,
) -> bool {
    let chord = zdt::keys::chord_of(event, modifiers).expect("it is a chord");
    terminals.terminal_key(vim, id, chord, event, modifiers)
}

#[test]
fn every_key_a_terminal_does_not_bind_is_the_programs() {
    // The whole point of terminal mode. A base map that took `<Esc>`, `<C-u>` or `<C-l>` would
    // make a shell unusable, so a binding written without a mode letter must not reach here.
    let window = Window::open();
    let (terminals, workspace) = mount(&window);
    let vim = modal(&window, &workspace);

    let id = window
        .scope
        .with(|| terminals.open(&waiting()))
        .expect("cat starts");

    let escape = KeyEvent::named(NamedKey::Escape, PhysicalKey::Unidentified(0));
    assert!(!press(&terminals, &vim, id, &escape, Modifiers::NONE));
    for letter in ["l", "u", "c", "w", "d", "r", "a", "e"] {
        assert!(
            !press(&terminals, &vim, id, &typed(letter), Modifiers::CONTROL),
            "<C-{letter}> is the program's"
        );
    }

    terminals.close(id);
}

#[test]
fn the_way_out_needs_both_keys() {
    let window = Window::open();
    let (terminals, workspace) = mount(&window);
    let vim = modal(&window, &workspace);

    let id = window
        .scope
        .with(|| terminals.open(&waiting()))
        .expect("cat starts");

    // `<C-\>` on its own is held: it is the start of a binding and says nothing yet.
    assert!(press(
        &terminals,
        &vim,
        id,
        &typed("\\"),
        Modifiers::CONTROL
    ));
    assert_eq!(
        terminals.mode_of(id),
        TerminalMode::Terminal,
        "and changes nothing by itself"
    );

    window.scope.with(|| {
        assert!(press(&terminals, &vim, id, &typed("n"), Modifiers::CONTROL));
    });
    assert_eq!(terminals.mode_of(id), TerminalMode::Normal);

    terminals.close(id);
}

#[test]
fn a_held_key_that_leads_nowhere_goes_to_the_program() {
    let window = Window::open();
    let (terminals, workspace) = mount(&window);
    let vim = modal(&window, &workspace);

    let id = window
        .scope
        .with(|| terminals.open(&waiting()))
        .expect("cat starts");

    assert!(press(
        &terminals,
        &vim,
        id,
        &typed("\\"),
        Modifiers::CONTROL
    ));
    assert!(
        !press(&terminals, &vim, id, &typed("x"), Modifiers::NONE),
        "`<C-\\>x` was two keys the program should have had"
    );
    assert_eq!(terminals.mode_of(id), TerminalMode::Terminal);

    // And the sequence is forgotten, so the next `<C-\><C-n>` still works.
    assert!(press(
        &terminals,
        &vim,
        id,
        &typed("\\"),
        Modifiers::CONTROL
    ));
    window.scope.with(|| {
        assert!(press(&terminals, &vim, id, &typed("n"), Modifiers::CONTROL));
    });
    assert_eq!(terminals.mode_of(id), TerminalMode::Normal);

    terminals.close(id);
}
