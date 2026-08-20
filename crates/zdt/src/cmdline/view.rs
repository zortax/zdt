//! The command line.
//!
//! One row where the status line is, because that is where vim puts it and because a command line
//! that floats over the text covers the thing the command is about.
//!
//! It takes the keyboard while it is open. `<CR>` runs, `<Esc>` gives up, and `<Up>` and `<Down>`
//! walk what was typed before. That history lives as long as the window and no longer, which makes
//! it useful and leaves nothing to manage.

use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui::prelude::*;
use zgui_ui_primitives::prelude::*;

use crate::cmdline::use_cmdline;

/// The row, drawn only while a command is being typed.
#[component]
pub fn CommandLine() -> impl IntoView {
    let cmdline = use_cmdline();
    let surface = NodeRef::new();
    let present = {
        let cmdline = cmdline.clone();
        Signal::derive_local(move || cmdline.is_open())
    };

    // It has the keys while it is open, and the region underneath takes them back when it closes.
    crate::focus::claim::claim(crate::focus::Overlay::CommandLine, present);

    view! {
        Presence(present = present, surface = surface) {
            {view! { Typing(surface = surface) }}
        }
    }
}

/// One command being typed.
#[component]
fn Typing(
    /// The row itself, whose exit animation says when it may be taken away.
    surface: NodeRef,
) -> impl IntoView {
    let leaving = use_presence();
    let cmdline = use_cmdline();
    let field = NodeRef::new();
    let value = RwSignal::new_local(cmdline.text());

    // Where the keyboard lands while this is the thing in front.
    crate::focus::claim::sink(
        crate::focus::Spot::Overlay(crate::focus::Overlay::CommandLine),
        crate::focus::Sink::Node(field),
    );

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
        row(
            class = "cmdline",
            node_ref = surface,
            attr:data-state = move || zdt_view::leaving_state(leaving)
        ) {
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
