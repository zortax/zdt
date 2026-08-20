//! The one-line question.
//!
//! A small floating input over the middle of the window. It takes the keyboard while it is open
//! and gives it back when it closes. That is the whole of its behaviour. `<CR>` answers, and
//! `<Esc>` gives up.

use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui::prelude::*;
use zgui_ui_primitives::prelude::*;

use crate::prompt::use_prompt;

/// The floating input, drawn only while something is being asked.
#[component]
pub fn Prompt() -> impl IntoView {
    let prompt = use_prompt();
    let surface = NodeRef::new();

    // The question it was asked, kept for the length of the exit: what is pending is cleared the
    // moment it is answered, and a field that read it directly would empty out as it left.
    let showing: RwSignal<Option<crate::prompt::Pending>, LocalStorage> = RwSignal::new_local(None);
    let follow = zgui::reactive::RenderEffect::new(move |_| {
        if let Some(pending) = prompt.pending() {
            showing.set(Some(pending));
        }
    });
    on_cleanup_local(move || drop(follow));

    let present = Signal::derive_local(move || prompt.pending().is_some());

    // It has the keys while it is open, and the region underneath takes them back when it closes.
    crate::focus::claim::claim(crate::focus::Overlay::Prompt, present);

    view! {
        Presence(present = present, surface = surface) {
            {move || {
                use zdt_view::Erase;
                match showing.get() {
                    Some(pending) => view! {
                        Asking(title = pending.title, start = pending.start, surface = surface)
                    }
                    .any(),
                    None => ().any(),
                }
            }}
        }
    }
}

/// One question, rebuilt whenever a different one is asked.
///
/// Built fresh each time, so the field starts out holding the right text. An input's value is
/// written into the element, and rebuilding is the plainest way to write a new one.
#[component]
fn Asking(
    /// What is being asked.
    title: String,
    /// What the field starts out holding.
    start: String,
    /// The panel itself, whose exit animation says when it may be taken away.
    surface: NodeRef,
) -> impl IntoView {
    let prompt = use_prompt();
    let leaving = use_presence();
    let node = NodeRef::new();
    let value = RwSignal::new_local(start.clone());

    // Where the keyboard lands while this is the thing in front.
    crate::focus::claim::sink(
        crate::focus::Spot::Overlay(crate::focus::Overlay::Prompt),
        crate::focus::Sink::Node(node),
    );

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
        column(
            class = "prompt",
            node_ref = surface,
            attr:data-state = move || zdt_view::leaving_state(leaving),
            a11y:role = Role::Dialog,
            a11y:label = title.clone()
        ) {
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
