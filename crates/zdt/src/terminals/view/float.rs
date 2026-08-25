//! The floating terminal, and the box it is in.

use crate::terminals::use_terminals;
use crate::terminals::view::EmulatorProps;
use crate::workspace::BufferId;
use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui_primitives::prelude::*;

/// The floating terminal, over everything.
#[component]
pub fn FloatingTerminal() -> impl IntoView {
    let terminals = use_terminals();
    let surface = NodeRef::new();

    // Which terminal it was, kept for the length of the exit: what is showing is cleared the
    // moment the float is hidden, and a float that read it directly would go blank as it left.
    let showing: RwSignal<Option<BufferId>, LocalStorage> = RwSignal::new_local(None);
    let follow = {
        let terminals = terminals.clone();
        zgui::reactive::RenderEffect::new(move |_| {
            if let Some(buffer) = terminals.showing() {
                showing.set(Some(buffer));
            }
        })
    };
    on_cleanup_local(move || drop(follow));

    let present = {
        let terminals = terminals.clone();
        Signal::derive_local(move || terminals.showing().is_some())
    };

    // The float has the keys while it is up, and the region underneath takes them back when it
    // goes. There is no handing back to forget: the claim follows what is shown.
    {
        let terminals = terminals.clone();
        crate::focus::claim::claim_named(Signal::derive_local(move || {
            terminals.showing().map(crate::focus::Overlay::Float)
        }));
    }

    view! {
        Presence(present = present, surface = surface) {
            {move || {
                use zdt_view::Erase;
                match showing.get() {
                    Some(buffer) => view! { Float(buffer = buffer, surface = surface) }.any(),
                    None => ().any(),
                }
            }}
        }
    }
}

/// The float's own box, which is what the exit animation runs on.
#[component]
pub(crate) fn Float(
    /// Which terminal is in it.
    buffer: BufferId,
    /// The box itself, whose exit animation says when it may be taken away.
    surface: NodeRef,
) -> impl IntoView {
    let leaving = use_presence();
    view! {
        box(
            class = "termfloat",
            node_ref = surface,
            attr:data-state = move || zdt_view::leaving_state(leaving)
        ) {
            Emulator(buffer = buffer)
        }
    }
}
