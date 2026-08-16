//! The history half: the commit graph and the commits beside it.

use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui::prelude::*;

use crate::use_gitui;

use crate::panel::List;
use crate::panel::ROW;
use crate::visible::keep_visible;

use crate::history::row::HistoryRowProps;

mod row;

/// How wide one lane of the commit graph is.
pub(crate) const LANE: f32 = 12.0;

/// The history half: the graph and the commits beside it.
#[component]
pub(crate) fn HistoryList() -> impl IntoView {
    let git = use_gitui();
    let port = NodeRef::new();

    let count = {
        let git = git.clone();
        Signal::derive_local(move || git.commits().len())
    };
    let visible = {
        let git = git.clone();
        keep_visible(port, move || git.at(List::History), ROW)
    };
    on_cleanup_local(move || drop(visible));

    let focused = git.clone();

    view! {
        row(class = "git__heading") {
            label {"History"}
            box(class = "fill") {}
            label(class = "muted") {{move || count.get().to_string()}}
        }
        VirtualList(
            class = "git__history",
            node_ref = port,
            count = count,
            row_size = ROW,
            label = "History",
            attr:data-focused = move || {
                (focused.list() == List::History).then(|| "true".to_owned())
            },
            row = move |index: usize| view! { HistoryRow(index = index) },
        )
    }
}
