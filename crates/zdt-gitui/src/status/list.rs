//! One of the two file lists.

use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui::prelude::*;

use crate::use_gitui;

use crate::panel::List;
use crate::panel::ROW;
use crate::status::FileRowProps;
use zdt_view::keep_visible;

/// One of the two file lists.
#[component]
pub(crate) fn FileList(
    /// Which one.
    which: List,
    /// What to call it.
    #[prop(into)]
    heading: String,
) -> impl IntoView {
    let git = use_gitui();
    let port = NodeRef::new();

    let count = {
        let git = git.clone();
        Signal::derive_local(move || {
            if which == List::Staged {
                git.staged().len()
            } else {
                git.unstaged().len()
            }
        })
    };
    let visible = {
        let git = git.clone();
        keep_visible(port, move || git.at(which), ROW)
    };
    on_cleanup_local(move || drop(visible));

    let focused = git.clone();

    view! {
        row(class = "git__heading") {
            label {{heading}}
            box(class = "fill") {}
            label(class = "muted") {{move || count.get().to_string()}}
        }
        VirtualList(
            class = "git__files",
            node_ref = port,
            count = count,
            row_size = ROW,
            label = "Files",
            attr:data-focused = move || (focused.list() == which).then(|| "true".to_owned()),
            row = move |index: usize| view! { FileRow(which = which, index = index) },
        )
    }
}
