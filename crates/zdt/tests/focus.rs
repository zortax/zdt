//! Where the keyboard is.
//!
//! One value answers it, so the assertions here are mostly about what a transition makes *false*.
//! Two regions both believing they have the keyboard was the shape of three separate defects, and
//! the model exists so that it cannot be written down.
//!
//! No view is built. What is asserted is the state every part of the interface reads.

use zdt::explorer::Explorer;
use zdt::focus::{Focus, Overlay};
use zdt::settings::Settings;
use zdt::terminals::{Program, Terminals};
use zdt::workspace::{Axis, Workspace};
use zdt_core::tree::Filter;
use zdt_core::{Config, Project};
use zgui_testkit_view::Window;

/// A workspace with a tree over it, both reading one focus.
///
/// The focus is published, because a claim looks it up while it is being made.
fn mount(window: &Window) -> (Workspace, Explorer) {
    window.scope.with(|| {
        let workspace = Workspace::new(Project::at("/tmp"));
        zdt::focus::provide(workspace.focus().clone());
        let explorer = Explorer::new("/tmp", Filter::default(), workspace.focus().clone());
        (workspace, explorer)
    })
}

/// An overlay that is up while the answer says so.
///
/// The real mechanism: what every modal in the interface registers, driven by the same signal it
/// draws itself from.
fn overlay(
    window: &Window,
    which: Overlay,
) -> zgui::reactive::RwSignal<bool, zgui::reactive::LocalStorage> {
    let present = window
        .scope
        .with(|| zgui::reactive::RwSignal::new_local(false));
    window.scope.with(|| {
        zdt::focus::claim::claim(
            which,
            zgui::reactive::Signal::derive_local(move || {
                zgui::reactive::prelude::Get::get(&present)
            }),
        );
    });
    present
}

/// Puts `present` where it is asked and runs the frame that answers it.
fn set(
    window: &Window,
    present: zgui::reactive::RwSignal<bool, zgui::reactive::LocalStorage>,
    up: bool,
) {
    zgui::reactive::prelude::Set::set(&present, up);
    window.frame();
}

/// A terminals layer over `workspace`.
fn terminals(window: &Window, workspace: &Workspace) -> Terminals {
    window
        .scope
        .with(|| Terminals::new(workspace.clone(), Settings::new(Config::default(), None)))
}

/// A program that sits there until it is shut down.
fn waiting() -> Program {
    Program::command("cat")
}

#[test]
fn the_tree_and_a_window_cannot_both_have_it() {
    // The defect this is about: clicking a pane left the tree believing it still had the keyboard,
    // and `<C-h>` then did nothing at all.
    let window = Window::open();
    let (workspace, explorer) = mount(&window);
    let focus = workspace.focus();

    window.scope.with(|| explorer.toggle());
    assert!(
        explorer.is_focused(),
        "opening the tree puts the keys in it"
    );
    assert_eq!(focus.current(), Focus::Tree);

    // What a click on a pane does.
    let pane = workspace.focused_untracked();
    workspace.focus_window(pane);

    assert_eq!(focus.current(), Focus::Window(pane));
    assert!(
        !explorer.is_focused(),
        "and the tree knows it has lost them"
    );
    assert!(explorer.is_open(), "without the panel going away");
}

#[test]
fn the_current_pane_survives_a_trip_into_the_tree() {
    // The tree opens a file into a split, so a split has to be named the whole time the keyboard is
    // somewhere else.
    let window = Window::open();
    let (workspace, explorer) = mount(&window);
    let focus = workspace.focus();

    let first = workspace.focused_untracked();
    let second = window.scope.with(|| workspace.split(Axis::Vertical));
    let second = second.expect("it splits");
    assert_eq!(focus.current(), Focus::Window(second), "into the new split");

    window.scope.with(|| explorer.focus());
    assert_eq!(focus.current(), Focus::Tree);
    assert_eq!(focus.window(), second, "and the split is still the one");

    explorer.unfocus();
    assert_eq!(focus.current(), Focus::Window(second));
    assert_ne!(second, first);
}

#[test]
fn closing_a_split_from_the_tree_leaves_the_keys_in_the_tree() {
    let window = Window::open();
    let (workspace, explorer) = mount(&window);
    let focus = workspace.focus();

    let first = workspace.focused_untracked();
    let second = window
        .scope
        .with(|| workspace.split(Axis::Vertical))
        .expect("it splits");

    window.scope.with(|| explorer.focus());
    window.scope.with(|| workspace.close_window_at(second));

    assert_eq!(focus.current(), Focus::Tree, "the keys stay where they are");
    assert_eq!(focus.window(), first, "and the pane that is left is named");
}

#[test]
fn an_overlay_gives_the_region_back() {
    // The defect this is about: a prompt and a rename box took the keyboard and never handed it
    // over, because handing it over was a call somebody had to remember to write.
    let window = Window::open();
    let (workspace, explorer) = mount(&window);
    let focus = workspace.focus();

    let prompt = overlay(&window, Overlay::Prompt);

    for region in [Focus::Tree, Focus::Window(workspace.focused_untracked())] {
        window.scope.with(|| match region {
            Focus::Tree => explorer.focus(),
            _ => explorer.unfocus(),
        });
        assert_eq!(focus.current(), region);

        set(&window, prompt, true);
        assert_eq!(focus.current(), Focus::Overlay(Overlay::Prompt));

        set(&window, prompt, false);
        assert_eq!(focus.current(), region, "and the region has them back");
    }
}

#[test]
fn the_innermost_overlay_is_the_one_with_the_keys() {
    let window = Window::open();
    let (workspace, _) = mount(&window);
    let focus = workspace.focus();
    let (prompt, picker) = (
        overlay(&window, Overlay::Prompt),
        overlay(&window, Overlay::Picker),
    );

    set(&window, prompt, true);
    set(&window, picker, true);
    assert_eq!(focus.current(), Focus::Overlay(Overlay::Picker));

    set(&window, picker, false);
    assert_eq!(
        focus.current(),
        Focus::Overlay(Overlay::Prompt),
        "and the one underneath is listening again"
    );
}

#[test]
fn a_terminal_nobody_is_looking_at_names_no_mode() {
    // The defect this is about: the status line read a single "some terminal is being typed into"
    // flag, so any terminal split anywhere made it say TERMINAL.
    let window = Window::open();
    let (workspace, _) = mount(&window);
    let terminals = terminals(&window, &workspace);
    let vim = window.scope.with(|| {
        zdt::vim::Vim::new(
            workspace.clone(),
            Settings::new(Config::default(), None),
            zdt::keymaps::Keymaps::new(),
        )
    });
    let focus = workspace.focus();

    // A split with a terminal in it, being typed into.
    let split = window
        .scope
        .with(|| workspace.split(Axis::Vertical))
        .expect("it splits");
    let id = window
        .scope
        .with(|| terminals.open(&waiting()))
        .expect("cat starts");

    assert_eq!(focus.current(), Focus::Window(split));
    assert_eq!(
        focus.mode(&vim, Some(&terminals), &workspace),
        zdt_vim::Mode::Terminal,
        "the terminal answers while it is the one being looked at"
    );

    // And away again, without leaving terminal mode first.
    window.scope.with(|| workspace.cycle_window(true));
    assert_ne!(focus.current(), Focus::Window(split));
    assert_eq!(
        focus.mode(&vim, Some(&terminals), &workspace),
        zdt_vim::Mode::Normal,
        "a terminal in a split nobody is looking at says nothing"
    );

    // Coming back finds it as it was left.
    assert_eq!(
        terminals.mode_of(id),
        zdt::terminals::TerminalMode::Terminal,
        "which it still remembers"
    );
    terminals.close(id);
}

#[test]
fn the_command_line_says_which_mode_it_is() {
    let window = Window::open();
    let (workspace, _) = mount(&window);
    let vim = window.scope.with(|| {
        zdt::vim::Vim::new(
            workspace.clone(),
            Settings::new(Config::default(), None),
            zdt::keymaps::Keymaps::new(),
        )
    });
    let focus = workspace.focus();

    let line = overlay(&window, Overlay::CommandLine);
    set(&window, line, true);
    assert_eq!(focus.mode(&vim, None, &workspace), zdt_vim::Mode::Command);
}
