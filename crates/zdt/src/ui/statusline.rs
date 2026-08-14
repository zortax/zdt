//! The strip across the bottom: what mode the editor is in, what is open, and where the caret is.
//!
//! Everything here is read from a signal and nothing is pushed into it, so the status line has no
//! state of its own and cannot disagree with what is on screen.

use zgui::prelude::*;
use zgui::{component, view};
use zgui_editor::CursorPos;

use crate::workspace::use_workspace;

/// The whole strip.
#[component]
pub fn StatusLine() -> impl IntoView {
    let workspace = use_workspace();

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
            let buffer = workspace.window(window).map(|state| state.current);
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

    view! {
        row(class = "statusline") {
            box(class = "statusline__mode", attr:data-mode = "normal") {"NORMAL"}

            label(class = "statusline__name nowrap") {{name}}
            label(class = "statusline__mark") {{move || if dirty() { "[+]" } else { "" }}}

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

            label(class = "statusline__spelling") {{spelling}}
            label(class = "statusline__type") {{file_type}}
            label(class = "statusline__pos") {{move || {
                let CursorPos { line, col } = position();
                format!("{}:{}", line + 1, col + 1)
            }}}
        }
    }
}
