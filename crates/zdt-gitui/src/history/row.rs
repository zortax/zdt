//! One commit, with its piece of the graph.

use zgui::prelude::*;
use zgui::{component, view};

use crate::use_gitui;
use zdt_view::Erase;

use crate::history::LANE;
use crate::labels::ago_short;
use crate::panel::List;

/// One commit, with its piece of the graph.
#[component]
pub(crate) fn HistoryRow(
    /// Where it is in the history.
    index: usize,
) -> impl IntoView {
    let git = use_gitui();

    let commit = {
        let git = git.clone();
        move || git.commits().get(index).cloned()
    };
    let placed = {
        let git = git.clone();
        move || git.rows().get(index).cloned()
    };
    let chosen = {
        let git = git.clone();
        move || git.list() == List::History && git.at(List::History) == index
    };

    let (width, lanes, merge) = (placed.clone(), placed.clone(), commit.clone());
    let (short, tint, summary, author, when) = (
        commit.clone(),
        placed,
        commit.clone(),
        commit.clone(),
        commit,
    );

    let press = {
        let git = git.clone();
        move |_: &mut EventCx<'_, events::PointerDown>| {
            git.focus();
            git.set_list(List::History);
            git.go_to(List::History, index);
        }
    };

    view! {
        row(
            class = "git__commit",
            on:pointer_down = press,
            attr:data-selected = move || chosen().then(|| "true".to_owned())
        ) {
            // The graph, as thin absolutely-placed boxes. Every line in a commit graph is
            // vertical, and a row of boxes lines up with the text beside it for free. A canvas
            // would have to be told where the text is.
            box(
                class = "git__graph",
                style:width = move || {
                    let columns = width().map_or(1, |one| one.width).max(1);
                    Some(format!("{}px", columns as f32 * LANE))
                }
            ) {
                {move || {
                    let Some(placed) = lanes() else {
                        return ().any();
                    };
                    let is_merge = merge().is_some_and(|one| one.is_merge());
                    (0..placed.width.max(1))
                        .map(|column| {
                            let carries = placed.edges.iter().any(|edge| edge.from == column);
                            view! {
                                box(
                                    class = "git__lane",
                                    attr:data-tint =
                                        Some(zdt_git::graph::lane_tint(column).to_string()),
                                    attr:data-line = carries.then(|| "true".to_owned()),
                                    attr:data-dot =
                                        (column == placed.lane).then(|| "true".to_owned()),
                                    attr:data-merge = (column == placed.lane && is_merge)
                                        .then(|| "true".to_owned()),
                                    style:left = Some(format!("{}px", column as f32 * LANE))
                                ) {}
                            }
                            .any()
                        })
                        .collect::<Vec<_>>()
                        .any()
                }}
            }
            label(
                class = "git__short",
                style:color = move || {
                    let lane = tint().map_or(0, |one| one.lane);
                    Some(format!(
                        "var(--zdt-git-lane-{})",
                        zdt_git::graph::lane_tint(lane)
                    ))
                }
            ) {
                {move || short().map(|one| one.short).unwrap_or_default()}
            }
            label(class = "git__summary nowrap") {
                {move || summary().map(|one| one.summary).unwrap_or_default()}
            }
            box(class = "fill") {}
            label(class = "git__author nowrap muted") {
                {move || author().map(|one| one.author).unwrap_or_default()}
            }
            label(class = "git__when muted") {
                {move || when().map(|one| ago_short(one.when)).unwrap_or_default()}
            }
        }
    }
}
