//! The panel's frame, and the tabs that choose a half.

use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui::prelude::*;

use crate::use_gitui;
use zdt_view::Erase;

use crate::branches::BranchesProps;
use crate::commit::CommitBoxProps;
use crate::diff::DiffPaneProps;
use crate::history::HistoryListProps;
use crate::panel::View;
use crate::status::StatusListProps;
use zdt_icons::{self as icons, IconProps};

/// The panel itself.
#[component]
pub fn GitPanel(
    /// Where to record the panel's own element.
    ///
    /// The embedder registers it wherever the keyboard should land: a tab is the contents of a
    /// window, and the modal is a layer over whatever had the keyboard. The panel cannot tell which
    /// of the two it is, so it does not guess.
    #[prop(optional)]
    element_ref: Option<NodeRef>,
) -> impl IntoView {
    let git = use_gitui();
    let node = element_ref.unwrap_or_default();

    // The keys. Everything the panel answers goes through the host's keymap, so `s` and `q` are
    // rows in a file somebody can change and not characters written into a match.
    let on_key = {
        let host = git.host();
        move |event: &mut EventCx<'_, events::KeyDown>| {
            if host.key(event, event.modifiers) {
                event.prevent_default();
            }
            // Whatever the panel did with it, whatever is behind must not also see it.
            event.stop_propagation();
        }
    };

    let (head, view, working, problem) = (git.clone(), git.clone(), git.clone(), git.clone());
    let (focused, taking) = (git.host(), git.host());

    view! {
        column(
            class = "git__body",
            node_ref = node,
            tabindex = Focus::Programmatic,
            attr:data-focused = move || focused.has_keyboard().then(|| "true".to_owned()),
            on:key_down = on_key,
            on:focus_in = move |_: &mut EventCx<'_, events::FocusIn>| taking.took_keyboard()
        ) {
            // The strip across the top: which branch, which half, and whether anything is being
            // read. One line, because the panel is mostly list and diff and everything else has to
            // earn its pixels.
            row(class = "git__bar") {
                row(class = "git__head") {
                    Icon(icon = icons::GIT_BRANCH, class = "icon--sm")
                    label(class = "nowrap") {{move || head.head()}}
                }
                box(class = "fill") {}
                row(class = "git__views") {
                    ViewTab(which = View::Status, label = "Status")
                    ViewTab(which = View::History, label = "History")
                }
                box(class = "fill") {}
                row(class = "git__state") {
                    {move || {
                        if working.is_working() {
                            view! { Spinner() }.any()
                        } else {
                            ().any()
                        }
                    }}
                    label(class = "git__problem nowrap") {
                        {move || problem.problem().unwrap_or_default()}
                    }
                }
            }

            row(class = "git__panes") {
                column(class = "git__side") { Branches() }
                column(class = "git__list") {
                    {move || match view.view() {
                        View::Status => view! { StatusList() }.any(),
                        View::History => view! { HistoryList() }.any(),
                    }}
                }
                column(class = "git__diff") { DiffPane() }
            }

            CommitBox()
        }
    }
}

/// One of the two halves, as a tab.
#[component]
pub(crate) fn ViewTab(
    /// Which half.
    which: View,
    /// What it is called.
    #[prop(into)]
    label: String,
) -> impl IntoView {
    let git = use_gitui();
    let (reading, pressing) = (git.clone(), git);

    view! {
        control(
            class = "git__view-tab",
            attr:data-selected = move || (reading.view() == which).then(|| "true".to_owned()),
            tabindex = Focus::Programmatic,
            // On the press, and not the release. A tab that waits for the button to come back up
            // feels like it is thinking about it.
            on:pointer_down = move |_| pressing.show(which)
        ) {
            {label}
        }
    }
}
