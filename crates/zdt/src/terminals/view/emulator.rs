//! One terminal, mounted.

use crate::settings::Settings;
use crate::terminals::{TerminalMode, use_terminals};
use crate::workspace::{BufferId, WindowId, use_workspace};
use zgui::prelude::*;
use zgui::{component, view};
use zgui_terminal::{
    GridPoint, Input, PointFilter, PointerPhase, TerminalConfig, TerminalHandle, TerminalProps,
};

/// One terminal.
///
/// Mounted once per buffer and kept: the program behind it is a process, and rebuilding the view
/// would be starting a second one.
///
/// # Where the keys go
///
/// Both of a terminal's modes answer here, and which one is [`TerminalMode`]'s to say. In terminal
/// mode the program reads everything the keymap has not bound in `t`, and `input` stays
/// [`Input::Live`] so that a key, its release and what an input method composes all reach it. In
/// terminal-normal mode the keymap reads them all and `input` is [`Input::Held`], so nothing
/// leaks past the vim engine to the program.
#[component]
pub fn Emulator(
    /// Which buffer it is.
    buffer: BufferId,
    /// Which window it is in, when it is in one. `None` while it floats, which is what makes it
    /// an overlay rather than a window's contents.
    #[prop(optional)]
    window: Option<WindowId>,
) -> impl IntoView {
    use zdt_view::Erase;

    let terminals = use_terminals();
    let workspace = use_workspace();
    let settings = zgui::reactive::use_local_context::<Settings>();

    // Borrowed, and never taken. The program belongs to the session, so this view drawing it a
    // second time draws the same shell rather than starting another.
    let Some(running) = terminals.running(buffer) else {
        return view! {
            box(class = "pane__buffer--pending") {
                label(class = "muted") {"this terminal has ended"}
            }
        }
        .any();
    };

    let node = NodeRef::new();
    let config = terminal_config(settings.as_ref());

    // How the keyboard reaches this terminal. A window with a terminal in it has no editor to hand
    // it to, so without this, moving here with `<C-l>` would move the focus and leave the keys
    // behind. A float says so as an overlay instead, because that is what it is.
    let spot = match window {
        Some(window) => crate::focus::Spot::Buffer(window, buffer),
        None => crate::focus::Spot::Overlay(crate::focus::Overlay::Float(buffer)),
    };
    // Inside, because the focusable element with the key handlers on it is the emulator's own and
    // this box is the place on the screen it is drawn in. The same element in both modes, so
    // leaving terminal mode moves no focus at all.
    crate::focus::claim::sink(spot, crate::focus::Sink::Inside(node));

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
        let vim = crate::vim::use_vim();
        Box::new(
            move |event: &zgui::vocab::KeyEvent, modifiers: zgui::vocab::Modifiers| {
                let Some(chord) = crate::keys::chord_of(event, modifiers) else {
                    return false;
                };
                match terminals.mode_of_untracked(buffer) {
                    TerminalMode::Terminal => {
                        terminals.terminal_key(&vim, buffer, chord, event, modifiers)
                    }
                    TerminalMode::Normal => terminals.normal_key(&vim, buffer, chord),
                }
            },
        ) as zgui_terminal::KeyFilter
    };

    // While the keymap is reading, the program reads nothing. Held rather than answered key by
    // key, because a release and a composition are typed too and never reach a key filter.
    let input = {
        let terminals = terminals.clone();
        Signal::derive_local(move || match terminals.mode_of(buffer) {
            TerminalMode::Terminal => Input::Live,
            TerminalMode::Normal => Input::Held,
        })
    };

    let on_point = {
        let terminals = terminals.clone();
        let focus = crate::focus::use_focus();
        let vim = crate::vim::use_vim();
        Box::new(move |at: GridPoint, phase: PointerPhase| {
            if terminals.mode_of_untracked(buffer) != TerminalMode::Normal {
                return false;
            }
            // The engine owns what is selected, so a gesture is a motion it is told about rather
            // than a second thing painting the grid.
            match phase {
                PointerPhase::Down => {
                    if let Some(window) = window {
                        focus.enter_window(window);
                    }
                    terminals.point_at(&vim, buffer, at, false);
                }
                PointerPhase::Move => terminals.point_at(&vim, buffer, at, true),
                PointerPhase::Up => {}
            }
            true
        }) as PointFilter
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
                let focus = crate::focus::use_focus();
                move |_| {
                    // A terminal in a split is that split being taken up. A float is already the
                    // overlay with the keys, so it says nothing about which split is current.
                    //
                    // Nothing here changes the mode: a press that started typing would throw away
                    // a caret somebody had put somewhere.
                    if let Some(window) = window {
                        focus.enter_window(window);
                    }
                }
            }
        ) {
            Terminal(
                class = "terminal__grid",
                session = running,
                config = config,
                input = input,
                on_ready = on_ready,
                on_title = on_title,
                on_exit = on_exit,
                on_key = on_key,
                on_point = on_point,
            )
        }
    }
    .any()
}

/// How a terminal looks and behaves, as the settings say.
pub(crate) fn terminal_config(settings: Option<&Settings>) -> TerminalConfig {
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
