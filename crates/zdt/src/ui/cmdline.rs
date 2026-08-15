//! The command line.
//!
//! One row where the status line is, because that is where vim puts it and because a command line
//! that floats over the text covers the thing the command is about.
//!
//! It takes the keyboard while it is open. `<CR>` runs, `<Esc>` gives up, `<Up>` and `<Down>` walk
//! what was typed before — a history that lives as long as the window and no longer, which is what
//! makes it useful without being a file to manage.

use std::time::Duration;

use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui::prelude::*;

use crate::cmdline::use_cmdline;

/// The row, drawn only while a command is being typed.
#[component]
pub fn CommandLine() -> impl IntoView {
    let cmdline = use_cmdline();

    view! {
        {move || {
            use crate::ui::Erase;
            match cmdline.is_open() {
                true => view! { Typing() }.any(),
                false => ().any(),
            }
        }}
    }
}

/// One command being typed.
#[component]
fn Typing() -> impl IntoView {
    let cmdline = use_cmdline();
    let field = NodeRef::new();
    let value = RwSignal::new_local(cmdline.text());

    let claim = zgui::view::time::Timers::current()
        .map(|timers| timers.set_timeout(Duration::ZERO, move || field.focus()));
    on_cleanup_local(move || drop(claim));

    // What is typed reaches the command line as it is typed, so the history walk can put
    // something else there and the field follows.
    let typing = {
        let cmdline = cmdline.clone();
        zgui::reactive::RenderEffect::new(move |_| cmdline.set_text(&value.get()))
    };
    on_cleanup_local(move || drop(typing));

    let on_key = {
        let cmdline = cmdline.clone();
        move |event: &mut EventCx<'_, events::KeyDown>| {
            match &event.key {
                Key::Named(NamedKey::Enter) => {
                    cmdline.submit();
                    event.prevent_default();
                }
                Key::Named(NamedKey::Escape) => {
                    cmdline.cancel();
                    event.prevent_default();
                }
                // Walking the history writes into the field, which is why the field's value is
                // bound to a signal the command line can also write.
                Key::Named(NamedKey::ArrowUp) => {
                    if let Some(older) = cmdline.older() {
                        value.set(older);
                    }
                    event.prevent_default();
                }
                Key::Named(NamedKey::ArrowDown) => {
                    value.set(cmdline.newer().unwrap_or_default());
                    event.prevent_default();
                }
                _ => {}
            }
            // Nothing else reaches the editor behind: every key belongs to the line while it is
            // open, including the ones the keymap would otherwise answer.
            event.stop_propagation();
        }
    };

    view! {
        row(class = "cmdline") {
            label(class = "cmdline__sigil") {":"}
            Input(
                class = "cmdline__input",
                node_ref = field,
                value = Binding::from(value),
                a11y:label = "Command",
                on:key_down = on_key,
            )
        }
    }
}
