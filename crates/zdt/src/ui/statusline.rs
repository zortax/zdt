//! The strip across the bottom: what mode the editor is in, what is open, and where the caret is.
//!
//! Everything here is read from a signal and nothing is pushed into it, so the status line has no
//! state of its own and cannot disagree with what is on screen.

use zgui::prelude::*;
use zgui::{component, view};

use crate::icons::{self, IconProps};
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
    // what makes this follow a `<C-w>` rather than the buffer.
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

    // A terminal being typed into is a mode the engine knows nothing about: it is not answering
    // while a program is, so what mode the editor is in says nothing about where the keys go.
    let mode = {
        let vim = vim.clone();
        let terminals = zgui::reactive::use_local_context::<crate::terminals::Terminals>();
        move || {
            if terminals
                .as_ref()
                .is_some_and(|terminals| terminals.typing().is_some())
            {
                return zdt_vim::Mode::Terminal;
            }
            vim.mode()
        }
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
            let parts = crate::ui::diagnostics::summary(language, Some(&path));
            (!parts.is_empty()).then_some(parts)
        }
    };
    // What state the servers for this file are in — which is a fact that stays true until it
    // changes, and so belongs here. What they have just *done* goes to the announcements: a status
    // line with one slot can only ever show the last of several things that happened, and showing
    // the last one is showing the wrong one as often as not.
    let state = {
        let (language, workspace) = (language.clone(), workspace.clone());
        move || {
            let language = language.as_ref()?;
            let path = workspace.current_buffer().and_then(|buffer| buffer.path);
            let state = language.state(path.as_deref());
            // Nothing claims this file: the status line says nothing rather than reserving a
            // space for a word about a thing that is not happening.
            (state != crate::language::ServerState::Inactive).then_some(state)
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
                        use crate::ui::Erase;
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
                    use crate::ui::Erase;
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
