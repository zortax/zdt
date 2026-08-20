//! One changed file.

use zgui::prelude::*;
use zgui::{component, view};

use crate::use_gitui;

use crate::panel::List;

use crate::labels::state_mark;

/// One changed file.
#[component]
pub(crate) fn FileRow(
    /// Which list it is in.
    which: List,
    /// Where it is in that list.
    index: usize,
) -> impl IntoView {
    let git = use_gitui();
    let entry = {
        let git = git.clone();
        move || {
            let entries = if which == List::Staged {
                git.staged()
            } else {
                git.unstaged()
            };
            entries.get(index).cloned()
        }
    };
    // Which side of the entry this list is showing decides which mark it gets: the same file can
    // be staged as a rename and changed again since.
    let state = {
        let entry = entry.clone();
        move || {
            entry().map(|one| {
                if which == List::Staged {
                    one.index
                } else {
                    one.worktree
                }
            })
        }
    };
    let (mark, tint) = (state.clone(), state);
    let (name, conflicted) = (entry.clone(), entry);
    let chosen = {
        let git = git.clone();
        move || git.list() == which && git.at(which) == index
    };

    let press = {
        let git = git.clone();
        move |_: &mut EventCx<'_, events::PointerDown>| {
            git.host().took_keyboard();
            git.set_list(which);
            git.go_to(which, index);
        }
    };

    view! {
        row(
            class = "git__file",
            on:pointer_down = press,
            attr:data-selected = move || chosen().then(|| "true".to_owned()),
            attr:data-conflicted = move || {
                conflicted()
                    .is_some_and(|one| one.is_conflicted())
                    .then(|| "true".to_owned())
            }
        ) {
            label(
                class = "git__file-mark",
                style:color = move || {
                    tint().map(|state| format!("var(--{})", state_mark(state).1))
                }
            ) {
                {move || mark().map_or(" ", |state| state_mark(state).0).to_owned()}
            }
            label(class = "git__file-name nowrap") {
                {move || name().map(|one| one.path).unwrap_or_default()}
            }
        }
    }
}
