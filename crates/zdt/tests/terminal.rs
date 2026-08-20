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
use zdt::terminals::{Program, Terminals};
use zdt::workspace::Workspace;
use zdt_core::{Config, Project};
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
    let (terminals, _) = mount(&window);

    window
        .scope
        .with(|| terminals.toggle_float("default", &waiting()));
    let id = terminals.showing().expect("it is showing");
    assert!(terminals.is_inserting(id), "and the keys go to it");

    terminals.stop_typing(id);
    assert!(!terminals.is_inserting(id), "`<C-\\><C-n>` takes them back");
    assert_eq!(
        terminals.showing(),
        Some(id),
        "and leaves the float where it is"
    );

    terminals.close(id);
}

#[test]
fn the_way_out_needs_both_keys() {
    let window = Window::open();
    let (terminals, _) = mount(&window);

    assert!(!terminals.expecting_normal());
    terminals.expect_normal();
    assert!(
        terminals.expecting_normal(),
        "`<C-\\>` on its own only waits"
    );
    terminals.clear_expectation();
    assert!(!terminals.expecting_normal(), "and any other key ends it");
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
    assert!(!terminals.is_inserting(id));
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
