//! The box a new name is typed into.

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::{component, view};
use zgui_ui::prelude::*;
use zgui_ui_primitives::prelude::*;

use super::{Asking, LAYER, use_rename};
use zdt_view::anchor::{Anchoring, place};

#[component]
pub fn RenameBox() -> impl IntoView {
    let rename = use_rename();
    let surface = NodeRef::new();

    // What it was asking, kept for the length of the exit.
    let showing: RwSignal<Option<Asking>, LocalStorage> = RwSignal::new_local(None);
    let follow = zgui::reactive::RenderEffect::new(move |_| {
        if let Some(what) = rename.asking() {
            showing.set(Some(what));
        }
    });
    on_cleanup_local(move || drop(follow));

    // The band under the symbol, put on while the box is open and taken off when it closes.
    let workspace = crate::workspace::use_workspace();
    let banding = zgui::reactive::RenderEffect::new(move |_| {
        let Some(handle) = workspace.current_handle() else {
            return;
        };
        match rename.asking() {
            Some(what) => handle.set_decorations(
                LAYER,
                vec![zgui_editor::decoration::Decoration {
                    range: what.range,
                    kind: zgui_editor::decoration::DecorationKind::Background(
                        zgui_editor::decoration::Paint::Property("zdt-match".into()),
                    ),
                }],
            ),
            None => handle.clear_decorations(LAYER),
        }
    });
    on_cleanup_local(move || drop(banding));

    view! {
        Presence(
            present = Signal::derive_local(move || rename.asking().is_some()),
            surface = surface
        ) {
            {move || {
                use zdt_view::Erase;
                match showing.get() {
                    Some(what) => view! { Box(asking = what, surface = surface) }.any(),
                    None => ().any(),
                }
            }}
        }
    }
}

/// One box, rebuilt whenever a different symbol is renamed.
///
/// Built fresh each time, so the field starts out holding the right text. An input's value is
/// written into the element, and rebuilding is the plainest way to write a new one.
#[component]
pub(crate) fn Box(
    /// What is being renamed.
    asking: Asking,
    /// The box itself, whose exit animation says when it may be taken away.
    surface: NodeRef,
) -> impl IntoView {
    let rename = use_rename();
    let workspace = crate::workspace::use_workspace();
    let leaving = use_presence();
    let node = NodeRef::new();
    let value = RwSignal::new_local(asking.name.clone());

    // The presence's handle is shared with every box before this one and nothing clears it when an
    // element goes away. Cleared before anything observes it, as `ui::hover::Panel` does.
    surface.unbind();

    let caret = asking.caret;
    let placed = place(
        surface,
        move || Some(caret),
        Anchoring::default().offset(4.0),
    );

    // From a timer for the same reason the prompt's is: nothing unmounted takes focus.
    let claim = zgui::view::time::Timers::current()
        .map(|timers| timers.set_timeout(std::time::Duration::ZERO, move || node.focus()));
    on_cleanup_local(move || drop(claim));

    let on_key = move |event: &mut EventCx<'_, events::KeyDown>| {
        match &event.key {
            Key::Named(NamedKey::Enter) => {
                rename.submit(&workspace, &value.get_untracked());
                event.prevent_default();
                event.stop_propagation();
            }
            Key::Named(NamedKey::Escape) => {
                rename.cancel();
                event.prevent_default();
                event.stop_propagation();
            }
            // Everything else is text going into the field, and must not reach the editor behind.
            _ => event.stop_propagation(),
        }
    };

    view! {
        row(
            class = "rename",
            node_ref = surface,
            attr:data-state = move || zdt_view::leaving_state(leaving),
            attr:data-side = move || placed.side.get(),
            style:left = placed.left_px(),
            style:top = placed.top_px(),
            style:visibility = placed.visibility(),
            a11y:role = Role::Dialog,
            a11y:label = "Rename"
        ) {
            Input(
                class = "rename__input",
                node_ref = node,
                value = Binding::from(value),
                a11y:label = "New name",
                on:key_down = on_key
            )
        }
    }
}
