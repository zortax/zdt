//! The splitter that sets how wide the panel is.

use crate::explorer::use_explorer;
use crate::workspace::use_workspace;
use zgui::prelude::*;
use zgui::{component, view};

/// How narrow and how wide the tree may be dragged, matching what `tree.css` will honour.
const NARROWEST: f32 = 160.0;
const WIDEST: f32 = 480.0;

/// The edge between the tree and the work area, as something to pull.
///
/// It draws nothing. The width it sets is [`crate::settings::Settings`]'s, so a drag is the same
/// change the settings page makes and is written to the configuration with it.
#[component]
pub fn TreeResize() -> impl IntoView {
    let explorer = use_explorer();
    let settings = crate::settings::use_settings();
    let workspace = use_workspace();

    // Where the pointer went down and how wide the tree was then. Taken as a delta, so nothing
    // here has to be measured or converted.
    let from: zgui::reactive::RwSignal<Option<(f32, f32)>, zgui::reactive::LocalStorage> =
        zgui::reactive::RwSignal::new_local(None);

    let width_now = {
        let settings = settings.clone();
        move || settings.with_untracked(|config| config.tree.width) as f32
    };
    let resize = {
        let settings = settings.clone();
        move |to: f32| {
            let to = to.clamp(NARROWEST, WIDEST).round() as u32;
            if settings.with_untracked(|config| config.tree.width) != to {
                settings.edit(move |config| config.tree.width = to);
            }
        }
    };

    view! {
        box(
            class = "tree__resize",
            attr:data-open = move || explorer.is_open().then(|| "true".to_owned()),
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
                // Dragging an edge is not asking for the keyboard.
                workspace.focus().reproject();
            },
            on:pointer_cancel = move |ev: &mut EventCx<'_, events::PointerCancel>| {
                from.set(None);
                ev.release_pointer();
            },
            a11y:role = Role::Splitter,
            a11y:label = "Tree width"
        ) {}
    }
}
