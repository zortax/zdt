//! The splitter that sets how wide the agent sidebar is.

use zgui::prelude::*;
use zgui::{component, view};

use crate::workspace::use_workspace;

/// The edge between the agent sidebar and whatever sits beside it.
///
/// It draws nothing. While it is pulled it moves the sidebar's live width, which the sidebar
/// draws as its own inline width. The setting is written once, when the pointer lets go, so a
/// drag is the same change the settings page makes and is written to the configuration with it.
#[component]
pub fn AgentResize() -> impl IntoView {
    let agent = zdt_agentui::use_agent();
    let settings = crate::settings::use_settings();
    let workspace = use_workspace();

    let from: zgui::reactive::RwSignal<Option<(f32, f32)>, zgui::reactive::LocalStorage> =
        zgui::reactive::RwSignal::new_local(None);

    let width = agent.side_width().clone();
    let let_go = {
        let (width, settings) = (width.clone(), settings.clone());
        move || {
            from.set(None);
            let to = width.end();
            if settings.with_untracked(|config| config.agent.width) != to {
                settings.edit(move |config| config.agent.width = to);
            }
        }
    };

    view! {
        box(
            class = "agent-side__resize",
            attr:data-open = move || agent.is_open().then(|| "true".to_owned()),
            on:pointer_down = {
                let width = width.clone();
                move |ev: &mut EventCx<'_, events::PointerDown>| {
                    width.begin();
                    from.set(Some((ev.position.x.0, width.get_untracked() as f32)));
                    ev.capture_pointer();
                }
            },
            on:pointer_move = {
                let width = width.clone();
                move |ev: &mut EventCx<'_, events::PointerMove>| {
                    if let Some((at, was)) = from.get_untracked() {
                        width.drag_to(was + (ev.position.x.0 - at));
                    }
                }
            },
            on:pointer_up = {
                let let_go = let_go.clone();
                move |ev: &mut EventCx<'_, events::PointerUp>| {
                    let_go();
                    ev.release_pointer();
                    workspace.focus().reproject();
                }
            },
            on:pointer_cancel = move |ev: &mut EventCx<'_, events::PointerCancel>| {
                let_go();
                ev.release_pointer();
            },
            a11y:role = Role::Splitter,
            a11y:label = "Agent sidebar width"
        ) {}
    }
}
