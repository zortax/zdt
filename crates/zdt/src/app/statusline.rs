//! The strip across the bottom: what mode the editor is in, what is open, and where the caret is.
//!
//! Everything here is read from a signal and nothing is pushed into it, so the status line has no
//! state of its own and cannot disagree with what is on screen.

use zgui::prelude::*;
use zgui::{component, view};

use zdt_icons::{self as icons, IconProps};
use zgui_editor::CursorPos;

use crate::vim::use_vim;
use crate::workspace::use_workspace;

/// The whole strip.
#[component]
pub fn StatusLine() -> impl IntoView {
    let workspace = use_workspace();
    let vim = use_vim();

    let buffer = {
        let workspace = workspace.clone();
        move || workspace.current_buffer()
    };

    let name = {
        let workspace = workspace.clone();
        let buffer = buffer.clone();
        move || match buffer() {
            Some(entry) => entry.label_in(workspace.project()),
            None => String::new(),
        }
    };

    let dirty = {
        let buffer = buffer.clone();
        move || buffer().is_some_and(|entry| entry.is_dirty())
    };

    // The same glyph the buffer line puts on the tab, and in the same colour. A status line that
    // named the file type in words *and* drew it would be saying one thing twice; the glyph says
    // it, and the word beside it at the other end of the line says which grammar is highlighting.
    let glyph = {
        let buffer = buffer.clone();
        move || match buffer() {
            Some(entry) => entry.file_type.glyph.to_owned(),
            None => String::new(),
        }
    };

    let tint = {
        let buffer = buffer.clone();
        move || buffer().map(|entry| format!("var(--{})", entry.file_type.tint))
    };

    let file_type = {
        let buffer = buffer.clone();
        move || match buffer() {
            Some(entry) => entry.language().unwrap_or("").to_owned(),
            None => String::new(),
        }
    };

    let spelling = {
        let buffer = buffer.clone();
        move || match buffer() {
            Some(entry) => format!("{}  {}", entry.encoding.label(), entry.line_ending.label()),
            None => String::new(),
        }
    };

    // The caret's place, from whichever editor has the keyboard. Reading the focused window is
    // what makes this follow a `<C-w>`.
    let position = {
        let workspace = workspace.clone();
        move || {
            let window = workspace.focused();
            let buffer = workspace.window(window).and_then(|state| state.current);
            let handle = buffer.and_then(|buffer| workspace.handle_for(window, buffer));
            handle
                .map(|handle| handle.cursor_position().get())
                .unwrap_or_default()
        }
    };

    let message = {
        let workspace = workspace.clone();
        move || workspace.message()
    };

    // Derived from where the keyboard is, and read from the same function the key filter routes on,
    // so what is shown and where a key goes cannot disagree. A terminal in a split nobody is looking
    // at names no mode at all.
    let mode = {
        let (vim, workspace) = (vim.clone(), workspace.clone());
        let focus = crate::focus::use_focus();
        let terminals = zgui::reactive::use_local_context::<crate::terminals::Terminals>();
        move || focus.mode(&vim, terminals.as_ref(), &workspace)
    };
    let pending = {
        let vim = vim.clone();
        move || {
            let recording = vim.recording();
            let pending = vim.pending();
            match (recording, pending.is_empty()) {
                (Some(name), true) => format!("recording @{name}"),
                (Some(name), false) => format!("{pending}   recording @{name}"),
                (None, _) => pending,
            }
        }
    };

    // What the servers say about this file, and what they are busy with. Both empty when there is
    // nothing to say, so the status line does not reserve space for a silence.
    let language = zgui::reactive::use_local_context::<crate::language::Language>();
    let diagnostics = {
        let (language, workspace) = (language.clone(), workspace.clone());
        move || {
            let language = language.as_ref()?;
            // Read first, so this follows what the servers say.
            let _ = language.revision();
            let path = workspace.current_buffer().and_then(|buffer| buffer.path)?;
            let parts = crate::language::diagnostics::summary(language, Some(&path));
            (!parts.is_empty()).then_some(parts)
        }
    };
    // What state the servers for this file are in. That is a fact which stays true until it
    // changes, so it belongs here. What they have just *done* goes to the announcements. A status
    // line has one slot and can only show the last of several things that happened, which is the
    // wrong one as often as not.
    let state = {
        let (language, workspace) = (language.clone(), workspace.clone());
        move || {
            let language = language.as_ref()?;
            let path = workspace.current_buffer().and_then(|buffer| buffer.path);
            let state = language.state(path.as_deref());
            // Nothing claims this file, so the status line says nothing. Reserving a space for a
            // word about a thing that is not happening helps nobody.
            (state != crate::language::ServerState::Inactive).then_some(state)
        }
    };

    // The agent segment: how many threads want a person, and how many still work. Facts that stay
    // true, so they belong here; what just happened goes to the announcements.
    let agent = zdt_agentui::try_use_agent();
    let attention = {
        let agent = agent.clone();
        move || {
            let agent = agent.as_ref()?;
            let threads = agent.client().threads();
            let waiting = threads
                .iter()
                .filter(|shell| {
                    shell.asking > 0
                        || shell.planned
                        || shell.state == zdt_agent::thread::ThreadState::Failed
                })
                .count();
            let working = threads.iter().filter(|shell| shell.is_working()).count();
            match (waiting, working) {
                (0, 0) => None,
                (0, busy) => Some((format!("{busy} working"), "busy")),
                (open, 0) => Some((format!("{open} waiting"), "waiting")),
                (open, busy) => {
                    Some((format!("{open} waiting \u{00b7} {busy} working"), "waiting"))
                }
            }
        }
    };

    view! {
        row(class = "statusline") {
            box(
                class = "statusline__mode",
                attr:data-mode = {
                    let mode = mode.clone();
                    move || Some(mode().tone().to_owned())
                }
            ) {
                {move || mode().label().to_string()}
            }

            row(class = "statusline__file") {
                label(class = "glyph", style:color = tint) {{glyph}}
                label(class = "statusline__name nowrap") {{name}}
                // Unsaved work, in the one slot that is always there so the name does not shift
                // when a file is first edited.
                box(class = "statusline__mark") {
                    {move || {
                        use zdt_view::Erase;
                        if dirty() {
                            view! { Icon(icon = icons::PENCIL, class = "icon--xs", label = "Modified") }
                                .any()
                        } else {
                            ().any()
                        }
                    }}
                }
            }

            row(class = "statusline__diagnostics") {
                {move || {
                    diagnostics()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|part| view! {
                            label(
                                class = "statusline__count nowrap",
                                attr:data-tone = Some(part.tone.to_owned())
                            ) {{part.text}}
                        })
                        .collect::<Vec<_>>()
                }}
            }

            box(class = "fill") {}


            label(
                class = "statusline__message",
                attr:data-error = {
                    let message = message.clone();
                    move || message().and_then(|said| said.error.then(|| "true".to_owned()))
                }
            ) {
                {move || message().map(|said| said.text).unwrap_or_default()}
            }

            box(class = "fill") {}

            // A dot and a word, on the right with the rest of what is *true about this buffer*.
            // No spinner: while a server is working there is a loading announcement in the corner
            // already turning one, and two spinners for one job is one too many.
            row(
                class = "statusline__lsp",
                attr:data-state = {
                    let state = state.clone();
                    move || state().map(|state| state.tone().to_owned())
                }
            ) {
                {move || {
                    use zdt_view::Erase;
                    match state() {
                        Some(state) => view! {
                            box(class = "statusline__dot") {}
                            label(class = "nowrap") {{state.label().to_owned()}}
                        }
                        .any(),
                        None => ().any(),
                    }
                }}
            }

            row(
                class = "statusline__agent",
                attr:data-tone = {
                    let attention = attention.clone();
                    move || attention().map(|(_, tone)| tone.to_owned())
                }
            ) {
                {move || {
                    use zdt_view::Erase;
                    match attention() {
                        Some((said, _)) => view! {
                            Icon(icon = icons::BOT, class = "icon--xs")
                            label(class = "nowrap") {{said}}
                        }
                        .any(),
                        None => ().any(),
                    }
                }}
            }

            label(class = "statusline__pending") {{pending}}
            label(class = "statusline__spelling") {{spelling}}
            label(class = "statusline__type") {{file_type}}
            row(class = "statusline__pos") {
                Icon(icon = icons::HASH, class = "icon--xs")
                label() {{move || {
                    let CursorPos { line, col } = position();
                    format!("{}:{}", line + 1, col + 1)
                }}}
            }
        }
    }
}
