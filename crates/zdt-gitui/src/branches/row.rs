//! One branch.

use zgui::prelude::*;
use zgui::{component, view};

use crate::use_gitui;

use crate::panel::List;

/// One branch.
#[component]
pub(crate) fn BranchRow(
    /// Where it is in the list.
    index: usize,
) -> impl IntoView {
    let git = use_gitui();
    let branch = {
        let git = git.clone();
        move || git.branches().get(index).cloned()
    };
    let (mark, name, current, remote) = (
        branch.clone(),
        branch.clone(),
        branch.clone(),
        branch.clone(),
    );
    let chosen = {
        let git = git.clone();
        move || git.list() == List::Branches && git.at(List::Branches) == index
    };

    let press = {
        let git = git.clone();
        move |_: &mut EventCx<'_, events::PointerDown>| {
            git.host().took_keyboard();
            git.set_list(List::Branches);
            git.go_to(List::Branches, index);
        }
    };

    view! {
        row(
            class = "git__branch",
            on:pointer_down = press,
            attr:data-selected = move || chosen().then(|| "true".to_owned()),
            attr:data-current = move || {
                current()
                    .is_some_and(|one| one.current)
                    .then(|| "true".to_owned())
            },
            attr:data-remote = move || {
                remote()
                    .is_some_and(|one| one.remote)
                    .then(|| "true".to_owned())
            }
        ) {
            // The one that is checked out carries a mark as well as a tone. A tone on its own is
            // something somebody has to be told about.
            label(class = "git__branch-mark") {
                {move || {
                    if mark().is_some_and(|one| one.current) {
                        "\u{25cf}"
                    } else {
                        " "
                    }
                }}
            }
            label(class = "nowrap") {{move || name().map(|one| one.name).unwrap_or_default()}}
        }
    }
}
