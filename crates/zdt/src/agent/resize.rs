//! The splitter that sets how wide the agent sidebar is.

use zgui::prelude::*;
use zgui::{component, view};

use crate::workspace::use_workspace;

/// How narrow and how wide the sidebar may be dragged.
const NARROWEST: f32 = 200.0;
const WIDEST: f32 = 480.0;

/// The edge between the agent sidebar and whatever sits beside it.
///
/// It draws nothing. The width it sets is the settings', so a drag is the same change the
/// settings page makes and is written to the configuration with it.
#[component]
pub fn AgentResize() -> impl IntoView {
    let agent = zdt_agentui::use_agent();
    let settings = crate::settings::use_settings();
    let workspace = use_workspace();

    let from: zgui::reactive::RwSignal<Option<(f32, f32)>, zgui::reactive::LocalStorage> =
        zgui::reactive::RwSignal::new_local(None);

    let width_now = {
        let settings = settings.clone();
        move || settings.with_untracked(|config| config.agent.width) as f32
    };
    let resize = {
        let settings = settings.clone();
        move |to: f32| {
            let to = to.clamp(NARROWEST, WIDEST).round() as u32;
            if settings.with_untracked(|config| config.agent.width) != to {
                settings.edit(move |config| config.agent.width = to);
            }
        }
    };

    view! {
        box(
            class = "agent-side__resize",
            attr:data-open = move || agent.is_open().then(|| "true".to_owned()),
            on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                from.set(Some((ev.position.x.0, width_now())));
                ev.capture_pointer();
            },
            on:pointer_move = move |ev: &mut EventCx<'_, events::PointerMove>| {
                if let Some((at, was)) = from.get_untracked() {
                    resize(was + (ev.position.x.0 - at));
                }
            },
            on:pointer_up = move |ev: &mut EventCx<'_, events::PointerUp>| {
                from.set(None);
                ev.release_pointer();
                workspace.focus().reproject();
            },
            on:pointer_cancel = move |ev: &mut EventCx<'_, events::PointerCancel>| {
                from.set(None);
                ev.release_pointer();
            },
            a11y:role = Role::Splitter,
            a11y:label = "Agent sidebar width"
        ) {}
    }
}
