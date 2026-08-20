//! The modal, and the box inside it.

use super::*;
use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui_primitives::prelude::{PresenceProps, use_presence};

/// The settings, floating.
#[component]
pub fn ConfigModal() -> impl IntoView {
    use zdt_view::Erase;

    // Nothing to show without the state, which is every test that mounts a piece of the interface
    // without the root above it.
    let Some(state) = use_config_modal() else {
        return view! { box() }.any();
    };
    let surface = NodeRef::new();
    let present = {
        let state = state.clone();
        Signal::derive_local(move || state.is_open())
    };

    // It has the keys while it is open, and the region underneath takes them back when it closes.
    crate::focus::claim::claim(crate::focus::Overlay::Settings, present);

    view! {
        Presence(
            present = present,
            surface = surface
        ) {
            box(
                class = "config__scrim",
                on:pointer_down = {
                    let state = state.clone();
                    move |_: &mut EventCx<'_, events::PointerDown>| state.close()
                }
            ) {}
            ConfigFloating(surface = surface)
        }
    }
    .any()
}

/// The modal's own box.
///
/// Its own component, so [`use_presence`] runs inside the presence. An attribute closure runs
/// later and would find none, and the box would have no `data-state` to animate on.
#[component]
pub(crate) fn ConfigFloating(
    /// The box itself, whose exit animation says when it may be taken away.
    surface: NodeRef,
) -> impl IntoView {
    let leaving = use_presence();
    let state = use_config_modal();
    let node = NodeRef::new();

    // Escape closes it, which means the panel has to hold the keyboard.
    crate::focus::claim::sink(
        crate::focus::Spot::Overlay(crate::focus::Overlay::Settings),
        crate::focus::Sink::Node(node),
    );

    let on_key = move |event: &mut EventCx<'_, events::KeyDown>| {
        if matches!(event.key, Key::Named(NamedKey::Escape)) {
            if let Some(state) = state.as_ref() {
                state.close();
            }
            event.prevent_default();
        }
        // Everything else belongs to whatever field is being typed into, and to nothing behind.
        event.stop_propagation();
    };

    view! {
        column(
            class = "config__modal",
            node_ref = node,
            tabindex = Focus::Programmatic,
            attr:data-state = move || zdt_view::leaving_state(leaving),
            on:key_down = on_key,
            a11y:role = Role::Dialog,
            a11y:label = "Settings"
        ) {
            box(node_ref = surface, class = "config__modal-body") {
                ConfigPanel()
            }
        }
    }
}
