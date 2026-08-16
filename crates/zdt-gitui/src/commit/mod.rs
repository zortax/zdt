//! The box a commit message is typed into.

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RenderEffect, RwSignal};
use zgui::{component, view};
use zgui_ui_primitives::prelude::*;

use crate::use_gitui;
use zdt_view::Erase;

use crate::commit::message::MessageProps;

mod message;

/// The box a commit message is typed into.
#[component]
pub(crate) fn CommitBox() -> impl IntoView {
    let git = use_gitui();
    let showing: RwSignal<Option<String>, LocalStorage> = RwSignal::new_local(None);

    let follow = {
        let git = git.clone();
        RenderEffect::new(move |_| {
            if let Some(message) = git.message() {
                showing.set(Some(message));
            }
        })
    };
    on_cleanup_local(move || drop(follow));

    let present = {
        let git = git.clone();
        Signal::derive_local(move || git.message().is_some())
    };
    let surface = NodeRef::new();

    view! {
        Presence(present = present, surface = surface) {
            {move || match showing.get() {
                Some(start) => view! { Message(start = start, surface = surface) }.any(),
                None => ().any(),
            }}
        }
    }
}
