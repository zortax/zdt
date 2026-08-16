//! The git panel, drawn.
//!
//! One component, shown two ways: inside a scrim as a modal, and inside a pane as a tab. They are
//! the same component and share the same state, so opening the tab from the modal keeps the commit
//! somebody was looking at.
//!
//! # The layout
//!
//! A list on the left, a diff on the right, and a strip of branches down the side. That shape does
//! for both halves of the panel — the status side lists files, the history side lists commits —
//! which is why one frame serves both and why `<Tab>` between them is the whole of the navigation.
//!
//! # Every list is virtual
//!
//! A history is thousands of commits and a commit's diff is thousands of lines. Building a row for
//! each to show thirty is what a list must never do: the first version of this panel did, and it
//! made the whole window stop answering for as long as it was open. So each list is a
//! [`VirtualList`] over a count, exactly as the picker's matches are, and the diff is flattened
//! into [`DiffRow`]s so that it *has* a count to be over.
//!
//! A virtual list scrolls itself for a pointer and knows nothing about a caret moved by a key, so
//! [`keep_visible`] does that part.
//!
//! # How it takes the keyboard
//!
//! Like the file tree: the panel is one tab stop, it claims focus when it is shown, and its
//! `key_down` hands the key to the `"git"` keymap region. None of this goes through the editor's
//! key filter — the editor does not have the keyboard while the panel does.

use std::cell::RefCell;
use std::rc::Rc;

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RenderEffect, RwSignal};
use zgui::view::time::{TimeoutHandle, Timers};
use zgui::{component, view};
use zgui_ui::prelude::*;
use zgui_ui_primitives::prelude::*;

use crate::gitui::{DiffRow, List, Selected, View, ago_short, state_mark, use_gitui};
use crate::icons::{self, IconProps};
use crate::ui::Erase;
use crate::vim::use_vim;

/// How tall one row of any of the lists is.
///
/// Declared rather than measured, because that is what a virtual list needs: the window is decided
/// before its rows are built, and a height taken from the rows would mean building all of them to
/// find out which to build.
const ROW: f32 = 22.0;

/// How tall one row of the diff is. Tighter than a list row, because a diff is read as text.
const DIFF_ROW: f32 = 17.0;

/// How wide one lane of the commit graph is.
const LANE: f32 = 12.0;

/// The modal.
#[component]
pub fn GitModal() -> impl IntoView {
    let git = use_gitui();
    let surface = NodeRef::new();
    let present = {
        let git = git.clone();
        Signal::derive_local(move || git.is_open())
    };

    view! {
        Presence(present = present, surface = surface) {
            box(class = "git__scrim") {}
            Floating(surface = surface)
        }
    }
}

/// The modal's own box.
///
/// Its own component so that [`use_presence`] runs *here* — inside the presence — rather than
/// inside an attribute closure that is called later, in a scope that has no presence in it and
/// answers `None`. Without that the panel has no `data-state` and never plays its exit.
#[component]
fn Floating(
    /// The box itself, whose exit animation says when it may be taken away.
    surface: NodeRef,
) -> impl IntoView {
    let leaving = use_presence();

    view! {
        column(
            class = "git git--modal",
            node_ref = surface,
            attr:data-state = move || crate::ui::leaving_state(leaving),
            a11y:role = Role::Dialog,
            a11y:label = "Git"
        ) {
            GitPanel()
        }
    }
}

/// The panel itself.
#[component]
pub fn GitPanel() -> impl IntoView {
    let git = use_gitui();
    let vim = use_vim();
    let node = NodeRef::new();

    // The keys. Everything the panel answers goes through its own keymap region, so `s` and `q`
    // are rows in a file somebody can change rather than characters written into a match.
    let on_key = {
        let vim = vim.clone();
        move |event: &mut EventCx<'_, events::KeyDown>| {
            if let Some(chord) = crate::keys::chord_of(event, event.modifiers)
                && vim.key_in_region(chord, "git")
            {
                event.prevent_default();
            }
            // Whatever the panel did with it, the editor behind must not also see it.
            event.stop_propagation();
        }
    };

    // The panel takes the keyboard when it is shown. From a timer for the same reason the tree's
    // is: a node that has not been mounted cannot take focus, and this effect's first run happens
    // while the element is still being built.
    let claim: Rc<RefCell<Option<TimeoutHandle>>> = Rc::new(RefCell::new(None));
    let focusing = {
        let (git, held) = (git.clone(), Rc::clone(&claim));
        RenderEffect::new(move |_| {
            if !git.is_focused() {
                return;
            }
            if let Some(timers) = Timers::current() {
                *held.borrow_mut() =
                    Some(timers.set_timeout(std::time::Duration::ZERO, move || node.focus()));
            }
        })
    };
    on_cleanup_local(move || {
        drop(focusing);
        drop(claim);
    });

    // As a tab there is no editor to hand the keyboard over, so the panel takes it whenever its
    // own pane is the one being looked at.
    let following = {
        let (git, workspace) = (git.clone(), crate::workspace::use_workspace());
        RenderEffect::new(move |_| {
            let showing = workspace
                .current_buffer()
                .is_some_and(|buffer| matches!(buffer.kind, crate::workspace::BufferKind::Git));
            if showing {
                git.focus();
            }
        })
    };
    on_cleanup_local(move || drop(following));

    let (head, view, working, problem, focused, taking) = (
        git.clone(),
        git.clone(),
        git.clone(),
        git.clone(),
        git.clone(),
        git.clone(),
    );

    view! {
        column(
            class = "git__body",
            node_ref = node,
            tabindex = Focus::Programmatic,
            attr:data-focused = move || focused.is_focused().then(|| "true".to_owned()),
            on:key_down = on_key,
            on:focus_in = move |_: &mut EventCx<'_, events::FocusIn>| taking.focus()
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

/// Keeps the row the caret is on inside `port`.
///
/// A virtual list scrolls itself when a pointer asks it to, and knows nothing about a caret moved
/// by a key. Without this, `j` past the bottom of the window moves a selection nobody can see.
///
/// Everything is worked out in device pixels, which is the space a scroll container measures and
/// is scrolled in; the row height is stated in CSS pixels and is converted once, here.
fn keep_visible(port: NodeRef, at: impl Fn() -> usize + 'static, row: f32) -> RenderEffect<()> {
    // Observed once, outside the effect: asking for an observation *inside* one registers a fresh
    // observer every time it re-runs, and this effect re-runs on every measurement.
    let measured = port.observe_border_box();

    RenderEffect::new(move |_| {
        let index = at();
        // Read so that the effect follows the container's size as well as the caret.
        let _ = measured.get();

        let position = port.scroll_position();
        let height = position.scrollport.height.0;
        if height <= 0.0 {
            return;
        }
        let scale = port.scale();
        let density = if scale.is_finite() && scale > 0.01 {
            scale
        } else {
            1.0
        };
        let row = row * density;

        let top = position.offset.y.0;
        let wanted = index as f32 * row;
        // Moved as little as possible: a caret walking down scrolls one row at the bottom edge and
        // one walking up scrolls one row at the top. Anything already visible moves nothing, so
        // reading a list with the pointer is not fought by the caret.
        let next = if wanted < top {
            wanted
        } else if wanted + row > top + height {
            wanted + row - height
        } else {
            return;
        };

        port.scroll_to(
            zgui::view::ScrollTarget::Offset(zgui::geom::Point::new(
                zgui::geom::DevicePx(position.offset.x.0),
                zgui::geom::DevicePx(next.max(0.0)),
            )),
            zgui::view::ScrollBehavior::Instant,
        );
    })
}

/// One of the two halves, as a tab.
#[component]
fn ViewTab(
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
            // On the press rather than the release: a tab that waits for the button to come back
            // up feels like it is thinking about it.
            on:pointer_down = move |_| pressing.show(which)
        ) {
            {label}
        }
    }
}

/// The branches down the side.
#[component]
fn Branches() -> impl IntoView {
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

/// One branch.
#[component]
fn BranchRow(
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
            git.focus();
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
            // The one that is checked out is marked rather than merely tinted: a tone on its own
            // is a thing somebody has to be told about.
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

/// The status half: what is not staged, then what is.
#[component]
fn StatusList() -> impl IntoView {
    view! {
        FileList(which = List::Unstaged, heading = "Changes")
        FileList(which = List::Staged, heading = "Staged")
    }
}

/// One of the two file lists.
#[component]
fn FileList(
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

/// One changed file.
#[component]
fn FileRow(
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
            git.focus();
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

/// The history half: the graph and the commits beside it.
#[component]
fn HistoryList() -> impl IntoView {
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

/// One commit, with its piece of the graph.
#[component]
fn HistoryRow(
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
            // The graph, as thin absolutely-placed boxes rather than a canvas: every line in a
            // commit graph is vertical, and a row of boxes lines up with the text beside it for
            // free — which a canvas would have to be told about.
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

/// The diff, and what it is a diff of.
#[component]
fn DiffPane() -> impl IntoView {
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

/// One row of the diff: a file's heading, a hunk's header, or a line.
#[component]
fn DiffLine(
    /// Where it is in the flattened diff.
    index: usize,
) -> impl IntoView {
    let git = use_gitui();

    let found = {
        let git = git.clone();
        move || git.diff_rows().get(index).cloned()
    };
    // Whether the caret is in the hunk this row belongs to, which is what `s` would stage. Marked
    // down the leading edge rather than as a tint across it: the tints across a diff already mean
    // added and removed, and a third would be a third thing to learn.
    let within = {
        let (git, found) = (git.clone(), found.clone());
        move || {
            if git.list() != List::Diff {
                return false;
            }
            let here = found().and_then(|row| row.hunk());
            let caret = git
                .diff_rows()
                .get(git.at(List::Diff))
                .and_then(DiffRow::hunk);
            here.is_some() && here == caret
        }
    };
    let caret = {
        let git = git.clone();
        move || git.list() == List::Diff && git.at(List::Diff) == index
    };
    let body = found;

    let press = {
        let git = git.clone();
        move |_: &mut EventCx<'_, events::PointerDown>| {
            git.focus();
            git.set_list(List::Diff);
            git.go_to(List::Diff, index);
        }
    };

    view! {
        box(
            class = "git__diff-row",
            on:pointer_down = press,
            attr:data-hunk = move || within().then(|| "true".to_owned()),
            attr:data-caret = move || caret().then(|| "true".to_owned())
        ) {
            {move || {
                let Some(row) = body() else {
                    return ().any();
                };
                match row {
                    DiffRow::File {
                        path,
                        added,
                        removed,
                        binary,
                    } => view! {
                        row(class = "git__diff-head") {
                            label(class = "git__file-name nowrap") {{path}}
                            box(class = "fill") {}
                            {binary.then(|| view! { label(class = "muted") {"binary"} })}
                            label(class = "git__added") {{format!("+{added}")}}
                            label(class = "git__removed") {{format!("\u{2212}{removed}")}}
                        }
                    }
                    .any(),
                    DiffRow::Hunk { header, .. } => view! {
                        label(class = "git__hunk-head nowrap") {{header}}
                    }
                    .any(),
                    DiffRow::Line {
                        kind,
                        text,
                        old,
                        new,
                        ..
                    } => {
                        let tone = match kind {
                            zdt_git::LineKind::Added => "added",
                            zdt_git::LineKind::Removed => "removed",
                            zdt_git::LineKind::Context => "context",
                        };
                        let mark = match kind {
                            zdt_git::LineKind::Added => "+",
                            zdt_git::LineKind::Removed => "-",
                            zdt_git::LineKind::Context => " ",
                        };
                        view! {
                            row(class = "git__line", attr:data-kind = Some(tone.to_owned())) {
                                // Both sides' numbers, because a diff is two files and knowing
                                // which line of *which* is half of reading one.
                                label(class = "git__line-number") {
                                    {old.map(|n| n.to_string()).unwrap_or_default()}
                                }
                                label(class = "git__line-number") {
                                    {new.map(|n| n.to_string()).unwrap_or_default()}
                                }
                                label(class = "git__line-mark") {{mark}}
                                label(class = "git__line-text") {{text}}
                            }
                        }
                        .any()
                    }
                }
            }}
        }
    }
}

/// What one commit is.
#[component]
fn CommitDetails(
    /// Which commit.
    commit: zdt_git::Commit,
) -> impl IntoView {
    let body = commit.body.clone();

    view! {
        column(class = "git__details") {
            row(class = "git__details-line") {
                label(class = "git__short") {{commit.short.clone()}}
                label(class = "git__summary nowrap") {{commit.summary.clone()}}
            }
            row(class = "git__details-line muted") {
                label(class = "nowrap") {{format!("{} <{}>", commit.author, commit.email)}}
                box(class = "fill") {}
                label {{crate::gitui::ago(commit.when)}}
            }
            {(!body.is_empty()).then(|| view! {
                box(class = "git__details-body") {{body}}
            })}
        }
    }
}

/// The box a commit message is typed into.
#[component]
fn CommitBox() -> impl IntoView {
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

/// One commit message being typed.
///
/// Built fresh rather than updated, so the field starts out holding the right text — the same
/// reason the prompt and the rename box are.
#[component]
fn Message(
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
