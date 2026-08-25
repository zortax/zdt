//! The branch strip down the side.

use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui::prelude::*;

use crate::use_gitui;

use crate::branches::row::BranchRowProps;
use crate::panel::List;
use crate::panel::ROW;
use zdt_view::keep_visible;

mod row;

/// The branches down the side.
#[component]
pub(crate) fn Branches() -> impl IntoView {
    let git = use_gitui();
    let port = NodeRef::new();

    let count = {
        let git = git.clone();
        Signal::derive_local(move || git.branches().len())
    };
    let visible = {
        let git = git.clone();
        keep_visible(port, move || git.at(List::Branches), ROW)
    };
    on_cleanup_local(move || drop(visible));

    let focused = git.clone();

    view! {
        label(class = "git__heading") {"Branches"}
        VirtualList(
            class = "git__branches",
            node_ref = port,
            count = count,
            row_size = ROW,
            label = "Branches",
            attr:data-focused = move || {
                (focused.list() == List::Branches).then(|| "true".to_owned())
            },
            row = move |index: usize| view! { BranchRow(index = index) },
        )
    }
}
