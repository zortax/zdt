//! The field the message is typed in.

use zgui::prelude::*;
use zgui::reactive::RwSignal;
use zgui::view::time::Timers;
use zgui::{component, view};
use zgui_ui::prelude::*;

use crate::use_gitui;

/// One commit message being typed.
///
/// Built fresh each time, so the field starts out holding the right text. The prompt and the
/// rename box are built the same way, for the same reason.
#[component]
pub(crate) fn Message(
    /// What it opens holding.
    start: String,
    /// The box itself.
    surface: NodeRef,
) -> impl IntoView {
    let git = use_gitui();
    let node = NodeRef::new();
    let value = RwSignal::new_local(start);

    let claim = Timers::current()
        .map(|timers| timers.set_timeout(std::time::Duration::ZERO, move || node.focus()));
    on_cleanup_local(move || drop(claim));

    let on_key = {
        let git = git.clone();
        move |event: &mut EventCx<'_, events::KeyDown>| match &event.key {
            // Enter makes a new line, because a commit message has a body; the modifier commits.
            // The other way round would make writing a proper message a fight.
            Key::Named(NamedKey::Enter) if event.modifiers.control() || event.modifiers.meta() => {
                git.commit(&value.get_untracked());
                event.prevent_default();
                event.stop_propagation();
            }
            Key::Named(NamedKey::Escape) => {
                git.cancel_commit();
                event.prevent_default();
                event.stop_propagation();
            }
            _ => event.stop_propagation(),
        }
    };

    view! {
        column(class = "git__commit-box", node_ref = surface) {
            row(class = "git__commit-head") {
                label {"Commit message"}
                box(class = "fill") {}
                label(class = "muted") {"^Enter to commit, Esc to give up"}
            }
            Textarea(
                class = "git__commit-input",
                node_ref = node,
                value = Binding::from(value),
                placeholder = "What changed, and why",
                a11y:label = "Commit message",
                on:key_down = on_key
            )
        }
    }
}
