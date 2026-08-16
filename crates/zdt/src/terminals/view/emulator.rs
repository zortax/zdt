//! One terminal, mounted.

use crate::settings::Settings;
use crate::terminals::use_terminals;
use crate::workspace::{BufferId, WindowId, use_workspace};
use std::time::Duration;
use zgui::prelude::*;
use zgui::{component, view};
use zgui_terminal::{TerminalConfig, TerminalHandle, TerminalProps};

/// One terminal.
///
/// Mounted once per buffer and kept: the program behind it is a process, and rebuilding the view
/// would be starting a second one.
#[component]
pub fn Emulator(
    /// Which buffer it is.
    buffer: BufferId,
    /// Whether it is the floating one. A window's contents otherwise.
    floating: bool,
    /// Which window it is in, when it is in one. `None` while it floats.
    #[prop(optional)]
    window: Option<WindowId>,
) -> impl IntoView {
    use zdt_view::Erase;

    let terminals = use_terminals();
    let workspace = use_workspace();
    let settings = zgui::reactive::use_local_context::<Settings>();

    // Taken, and never borrowed. A program is started once, and a second view asking for the
    // same one is a mistake that would start a second shell.
    let Some(transport) = terminals.take_pending(buffer) else {
        return view! {
            box(class = "pane__buffer--pending") {
                label(class = "muted") {"this terminal has ended"}
            }
        }
        .any();
    };

    let node = NodeRef::new();
    let config = terminal_config(settings.as_ref());

    // The keyboard follows whichever terminal is being typed into, claimed from a timer for the
    // same reason every other claim in this application is: an unmounted node cannot take focus.
    let claiming = {
        let terminals = terminals.clone();
        let timers = zgui::view::time::Timers::current();
        let held: std::cell::RefCell<Option<zgui::view::time::TimeoutHandle>> =
            std::cell::RefCell::new(None);
        zgui::reactive::RenderEffect::new(move |_| {
            if terminals.typing() != Some(buffer) {
                return;
            }
            let Some(timers) = timers.as_ref() else {
                return;
            };
            *held.borrow_mut() = Some(timers.set_timeout(Duration::ZERO, move || node.focus()));
        })
    };
    on_cleanup_local(move || drop(claiming));

    // A window with a terminal in it has no editor to give the keyboard to, so nothing would hand
    // it over: moving here with `<C-l>` would move the focus and leave the keys behind. Typing
    // starts on arrival, which is what opening one already does; `<C-\><C-n>` and `<C-h/j/k/l>`
    // are the ways back out.
    let following = {
        let workspace = workspace.clone();
        let terminals = terminals.clone();
        zgui::reactive::RenderEffect::new(move |_| {
            let Some(window) = window else { return };
            if workspace.focused() == window
                && workspace
                    .window(window)
                    .is_some_and(|state| state.current == Some(buffer))
            {
                terminals.start_typing(buffer);
            }
        })
    };
    on_cleanup_local(move || drop(following));

    let on_ready = {
        let terminals = terminals.clone();
        Box::new(move |handle: TerminalHandle| terminals.register(buffer, handle))
            as Box<dyn Fn(TerminalHandle)>
    };

    let on_title = {
        let workspace = workspace.clone();
        Box::new(move |title: Option<String>| {
            workspace.rename_terminal(buffer, title);
        }) as Box<dyn Fn(Option<String>)>
    };

    let on_exit = {
        let workspace = workspace.clone();
        let terminals = terminals.clone();
        Box::new(move |_status: zgui_terminal::transport::ExitStatus| {
            // The program ended, so the buffer has nothing left to show. Vim keeps the last screen
            // until a key is pressed; this closes, because a buffer nobody can type into is one
            // more thing on the buffer line to close by hand.
            terminals.end(&workspace, buffer);
        }) as Box<dyn Fn(_)>
    };

    let on_key = {
        let terminals = terminals.clone();
        let workspace = workspace.clone();
        let vim = crate::vim::use_vim();
        Box::new(
            move |event: &zgui::vocab::KeyEvent, modifiers: zgui::vocab::Modifiers| {
                let Some(chord) = crate::keys::chord_of(event, modifiers) else {
                    return false;
                };
                if escape(&terminals, &workspace, &vim, chord, buffer, floating) {
                    return true;
                }
                if terminals.typing_untracked() == Some(buffer) {
                    // Being typed into. The keymap is consulted in terminal mode, where almost
                    // nothing is bound, so almost every key reaches the program. What is bound
                    // there wins on purpose: `<F7>` and the rest of vim's own `maps.t`.
                    return vim.key_in_region_as(chord, "terminal", zdt_vim::Mode::Terminal);
                }
                // Idle. The keymap answers in normal mode, so the scrollback can be walked and
                // `<Leader>ff` still works from inside a terminal.
                vim.key_in_region(chord, "terminal");
                true
            },
        ) as zgui_terminal::KeyFilter
    };

    // Whether *this* window is showing this terminal. Asking about the focused window instead
    // blanks every terminal in a split that does not have the keyboard.
    let current = {
        let workspace = workspace.clone();
        move || match window {
            None => true,
            Some(window) => workspace
                .window(window)
                .is_some_and(|state| state.current == Some(buffer)),
        }
    };

    view! {
        box(
            class = "terminal",
            node_ref = node,
            style:display = move || (!current()).then(|| "none".to_owned()),
            on:pointer_down = {
                let terminals = terminals.clone();
                move |_| terminals.start_typing(buffer)
            }
        ) {
            Terminal(
                class = "terminal__grid",
                transport = transport,
                config = config,
                on_ready = on_ready,
                on_title = on_title,
                on_exit = on_exit,
                on_key = on_key,
            )
        }
    }
    .any()
}

/// The keys a terminal does not get to keep.
///
/// Answers `true` when the key was one of them, which is what stops it reaching the program.
fn escape(
    terminals: &crate::terminals::Terminals,
    workspace: &crate::workspace::Workspace,
    vim: &crate::vim::Vim,
    chord: zdt_vim::Chord,
    buffer: BufferId,
    floating: bool,
) -> bool {
    use zdt_vim::chord::{Key, Mods};

    let control = chord.mods.contains(Mods::CONTROL);

    // `<C-\>` starts vim's way out, and the `<C-n>` after it finishes the job. Held on the
    // layer, because the engine is silent while a terminal is answering.
    if control && chord.key == Key::Char('\\') {
        terminals.expect_normal();
        return true;
    }
    if terminals.expecting_normal() {
        terminals.clear_expectation();
        if control && chord.key == Key::Char('n') {
            terminals.stop_typing();
            if floating {
                // A float with the keys taken away is a float nobody can reach: put it away and
                // give the editor back, which is what somebody pressing this meant.
                terminals.hide_float();
            }
            return true;
        }
        // `<C-\>` followed by anything else was `<C-\>` and then that thing: the program should
        // have had both, and has only missed the first.
        if let Some(handle) = terminals.handle(buffer) {
            handle.write(vec![0x1c]);
        }
        return false;
    }

    // Moving out of a terminal without leaving terminal mode first, as the user's own `maps.t`
    // does.
    if control && let Key::Char(letter @ ('h' | 'j' | 'k' | 'l')) = chord.key {
        terminals.stop_typing();
        if floating {
            terminals.hide_float();
        } else {
            workspace.cycle_window(matches!(letter, 'j' | 'l'));
        }
        vim.reset();
        return true;
    }

    false
}

/// How a terminal looks and behaves, as the settings say.
fn terminal_config(settings: Option<&Settings>) -> TerminalConfig {
    let (family, size, scrollback) = settings.map_or_else(
        || ("Mononoki Nerd Font".to_owned(), 13.0, 10_000),
        |settings| {
            settings.with_untracked(|config| {
                (
                    config.editor.font.clone(),
                    config.editor.font_size,
                    config.terminal.scrollback,
                )
            })
        },
    );

    TerminalConfig {
        // The mono variant first: a nerd font ships both, and the proportional one puts a terminal
        // grid out of alignment on every glyph that is not a letter.
        font_family: vec![
            format!("{family} Mono"),
            family,
            "DejaVu Sans Mono".to_owned(),
        ],
        font_size: size,
        line_height: 1.2,
        padding: 6.0,
        scrollback,
        copy_on_select: true,
        ..TerminalConfig::default()
    }
}
