//! The diff of whatever is selected.

use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui::prelude::*;

use crate::use_gitui;
use zdt_view::Erase;

use crate::panel::{List, Selected};

use crate::diff::line::DiffLineProps;
use zdt_view::keep_visible;

mod details;
mod line;
mod rows;

pub(crate) use crate::diff::details::CommitDetailsProps;
pub use crate::diff::rows::{DiffRow, diff_rows};

/// How tall one row of the diff is. Tighter than a list row, because a diff is read as text.
pub(crate) const DIFF_ROW: f32 = 17.0;

/// The diff, and what it is a diff of.
#[component]
pub(crate) fn DiffPane() -> impl IntoView {
    let git = use_gitui();
    let port = NodeRef::new();

    let count = {
        let git = git.clone();
        Signal::derive_local(move || git.diff_rows().len())
    };
    let visible = {
        let git = git.clone();
        keep_visible(port, move || git.at(List::Diff), DIFF_ROW)
    };
    on_cleanup_local(move || drop(visible));

    let (header, focused, split) = (git.clone(), git.clone(), git);

    view! {
        {move || match header.selected() {
            Selected::Commit(_) => match header.current_commit() {
                Some(commit) => view! { CommitDetails(commit = commit) }.any(),
                None => ().any(),
            },
            _ => ().any(),
        }}

        {move || {
            if count.get() == 0 {
                view! {
                    box(class = "git__empty") { label(class = "muted") {"nothing to show"} }
                }
                .any()
            } else {
                ().any()
            }
        }}

        VirtualList(
            class = "git__diff-body",
            node_ref = port,
            count = count,
            row_size = DIFF_ROW,
            label = "Diff",
            attr:data-focused = move || {
                (focused.list() == List::Diff).then(|| "true".to_owned())
            },
            attr:data-split = move || split.is_side_by_side().then(|| "true".to_owned()),
            row = move |index: usize| view! { DiffLine(index = index) },
        )
    }
}
