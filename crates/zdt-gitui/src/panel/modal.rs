//! The modal presentation: a scrim, and the panel inside it.

use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui_primitives::prelude::*;

use crate::use_gitui;

use crate::panel::GitPanelProps;

/// The modal.
#[component]
pub fn GitModal() -> impl IntoView {
    let git = use_gitui();
    let surface = NodeRef::new();
    let present = {
        let git = git.clone();
        Signal::derive_local(move || git.is_open())
    };

    view! {
        Presence(present = present, surface = surface) {
            box(class = "git__scrim") {}
            Floating(surface = surface)
        }
    }
}

/// The modal's own box.
///
/// Its own component, so that [`use_presence`] runs *here*, inside the presence. An attribute
/// closure runs later, in a scope with no presence in it, and answers `None`. The panel would then
/// have no `data-state` and would never play its exit.
#[component]
pub(crate) fn Floating(
    /// The box itself, whose exit animation says when it may be taken away.
    surface: NodeRef,
) -> impl IntoView {
    let leaving = use_presence();

    view! {
        column(
            class = "git git--modal",
            node_ref = surface,
            attr:data-state = move || zdt_view::leaving_state(leaving),
            a11y:role = Role::Dialog,
            a11y:label = "Git"
        ) {
            GitPanel()
        }
    }
}
