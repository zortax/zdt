//! The one-line question.
//!
//! A small floating input over the middle of the window. It takes the keyboard while it is open
//! and gives it back when it closes, which is the whole of its behaviour — `<CR>` answers, `<Esc>`
//! does not.

use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui::prelude::*;

use crate::prompt::use_prompt;

/// The floating input, drawn only while something is being asked.
#[component]
pub fn Prompt() -> impl IntoView {
    let prompt = use_prompt();

    view! {
        {move || {
            use crate::ui::Erase;
            match prompt.pending() {
                Some(pending) => {
                    view! { Asking(title = pending.title, start = pending.start) }.any()
                }
                None => ().any(),
            }
        }}
    }
}

/// One question, rebuilt whenever a different one is asked.
///
/// Built fresh rather than updated, so the field starts out holding the right text: an input's
/// value is written into the element, and rebuilding is the plainest way to write a new one.
#[component]
fn Asking(
    /// What is being asked.
    title: String,
    /// What the field starts out holding.
    start: String,
) -> impl IntoView {
    let prompt = use_prompt();
    let node = NodeRef::new();
    let value = RwSignal::new_local(start.clone());

    // From a timer for the same reason the tree's is: nothing unmounted takes focus.
    let claim = zgui::view::time::Timers::current()
        .map(|timers| timers.set_timeout(std::time::Duration::ZERO, move || node.focus()));
    on_cleanup_local(move || drop(claim));

    let on_key = move |event: &mut EventCx<'_, events::KeyDown>| {
        match &event.key {
            Key::Named(NamedKey::Enter) => {
                prompt.submit(&value.get_untracked());
                event.prevent_default();
                event.stop_propagation();
            }
            Key::Named(NamedKey::Escape) => {
                prompt.cancel();
                event.prevent_default();
                event.stop_propagation();
            }
            // Everything else is text going into the field, and must not reach the editor behind.
            _ => event.stop_propagation(),
        }
    };

    view! {
        column(class = "prompt", a11y:role = Role::Dialog, a11y:label = title.clone()) {
            label(class = "prompt__title nowrap") {{title}}
            Input(
                class = "prompt__input",
                node_ref = node,
                value = Binding::from(value),
                a11y:label = "Answer",
                on:key_down = on_key
            )
        }
    }
}
