//! The splitter that sets how wide the panel is.

use crate::explorer::use_explorer;
use crate::workspace::use_workspace;
use zgui::prelude::*;
use zgui::{component, view};

/// The edge between the tree and the work area, as something to pull.
///
/// It draws nothing. While it is pulled it moves the panel's live width, which the panel draws
/// as its own inline width. The setting is written once, when the pointer lets go, so a drag is
/// the same change the settings page makes and is written to the configuration with it.
#[component]
pub fn TreeResize() -> impl IntoView {
    let explorer = use_explorer();
    let settings = crate::settings::use_settings();
    let workspace = use_workspace();

    // Where the pointer went down and how wide the tree was then. Taken as a delta, so nothing
    // here has to be measured or converted.
    let from: zgui::reactive::RwSignal<Option<(f32, f32)>, zgui::reactive::LocalStorage> =
        zgui::reactive::RwSignal::new_local(None);

    let width = explorer.width().clone();
    let let_go = {
        let (width, settings) = (width.clone(), settings.clone());
        move || {
            from.set(None);
            let to = width.end();
            if settings.with_untracked(|config| config.tree.width) != to {
                settings.edit(move |config| config.tree.width = to);
            }
        }
    };

    view! {
        box(
            class = "tree__resize",
            attr:data-open = move || explorer.is_open().then(|| "true".to_owned()),
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
                    // Dragging an edge is not asking for the keyboard.
                    workspace.focus().reproject();
                }
            },
            on:pointer_cancel = move |ev: &mut EventCx<'_, events::PointerCancel>| {
                let_go();
                ev.release_pointer();
            },
            a11y:role = Role::Splitter,
            a11y:label = "Tree width"
        ) {}
    }
}
