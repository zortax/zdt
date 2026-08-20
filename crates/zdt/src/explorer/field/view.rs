//! The box a name is typed into, beside the row it is about.

use zdt_icons::IconProps;
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::{component, view};
use zgui_ui::prelude::*;
use zgui_ui_primitives::prelude::*;

use super::{Asking, At, use_field};
use crate::explorer::use_explorer;
use zdt_view::anchor::{AnchorRect, Anchoring, place};

/// The field.
#[component]
pub fn TreeField() -> impl IntoView {
    let field = use_field();
    let surface = NodeRef::new();

    // What it was asking, kept for the length of the exit: what is being asked is cleared the
    // moment the field closes, and one that read it directly would empty as it left.
    let showing: RwSignal<Option<Asking>, LocalStorage> = RwSignal::new_local(None);
    let follow = zgui::reactive::RenderEffect::new(move |_| {
        if let Some(what) = field.asking() {
            showing.set(Some(what));
        }
    });
    on_cleanup_local(move || drop(follow));

    // It has the keys while it is open, and the tree underneath takes them back when it closes.
    crate::focus::claim::claim(
        crate::focus::Overlay::TreeField,
        Signal::derive_local(move || field.asking().is_some()),
    );

    view! {
        Presence(
            present = Signal::derive_local(move || field.asking().is_some()),
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

/// One field, rebuilt whenever a different question is asked.
///
/// Built fresh each time, so the input starts out holding the right text. An input's value is
/// written into the element, and rebuilding is the plainest way to write a new one.
#[component]
fn Box(
    /// What is being asked.
    asking: Asking,
    /// The field itself, whose exit animation says when it may be taken away.
    surface: NodeRef,
) -> impl IntoView {
    let field = use_field();
    let explorer = use_explorer();
    let leaving = use_presence();
    let node = NodeRef::new();
    let value = RwSignal::new_local(asking.start.clone());
    let about = asking.about;

    // The presence's handle is shared with every field before this one and nothing clears it when
    // an element goes away. Cleared before anything observes it.
    surface.unbind();

    // Under the row it is about, or at the point the pointer went down. The solver flips it above
    // near the window's bottom edge and slides it back inside at the sides, and writes which way
    // it went into `data-side`.
    let at = asking.at;
    let placed = place(
        surface,
        move || match at {
            At::Row(row) => explorer.viewport()?.row_rect(row),
            At::Pointer(x, y) => Some(AnchorRect::point(x, y)),
        },
        Anchoring::on(Placement::new(Side::Bottom, Align::Start)).offset(2.0),
    );

    // Where the keyboard lands while this is the thing in front.
    crate::focus::claim::sink(
        crate::focus::Spot::Overlay(crate::focus::Overlay::TreeField),
        crate::focus::Sink::Node(node),
    );

    let on_key = move |event: &mut EventCx<'_, events::KeyDown>| {
        match &event.key {
            Key::Named(NamedKey::Enter) => {
                field.submit(&value.get_untracked());
                event.prevent_default();
                event.stop_propagation();
            }
            Key::Named(NamedKey::Escape) => {
                field.cancel();
                event.prevent_default();
                event.stop_propagation();
            }
            // Everything else is text going into the field, and must not reach the tree behind.
            _ => event.stop_propagation(),
        }
    };

    view! {
        row(
            class = "treefield",
            node_ref = surface,
            attr:data-state = move || zdt_view::leaving_state(leaving),
            attr:data-side = move || placed.side.get(),
            style:left = placed.left_px(),
            style:top = placed.top_px(),
            style:visibility = placed.visibility(),
            a11y:role = Role::Dialog,
            a11y:label = about.label()
        ) {
            Icon(icon = about.icon(), class = "icon--xs")
            Input(
                class = "treefield__input",
                node_ref = node,
                value = Binding::from(value),
                a11y:label = about.label(),
                on:key_down = on_key
            )
        }
    }
}
