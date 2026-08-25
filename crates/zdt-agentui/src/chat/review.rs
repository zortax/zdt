//! The diff review surface: a span of changes, line by line.
//!
//! Shown in the timeline's place while a review is open. The rows are flat: file heads, hunk
//! heads, and lines, derived in one pass from the loaded diffs and drawn by a virtual list, so
//! a review of any size builds only the window on screen. `j`/`k` walk the files, `<CR>` opens
//! the caret's file in the editor, `s` lays old and new side by side, `w` hides hunks that only
//! move whitespace, and `q` goes back to the conversation.

use std::collections::HashSet;
use std::rc::Rc;

use zdt_git::{DiffHunk, FileDiff, LineKind};
use zdt_icons::{self as icons, IconProps};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::{component, view};
use zgui_ui::prelude::*;

use crate::use_agent;

/// How tall one row of the review is, in CSS pixels.
const ROW: f32 = 17.0;

/// One row of the flattened review: a place in the loaded diffs.
///
/// A place and nothing more. The text stays in the shared files, so flattening a very large
/// diff copies nothing and a row reads what it draws when it is on screen.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
    /// One file's head.
    File {
        /// Which file.
        file: usize,
    },
    /// One hunk's `@@` line.
    Hunk {
        /// Which file.
        file: usize,
        /// Which of its hunks.
        hunk: usize,
    },
    /// One line.
    Line {
        /// Which file.
        file: usize,
        /// Which of its hunks.
        hunk: usize,
        /// Which of the hunk's lines.
        line: usize,
    },
}

/// Whether a hunk only moves whitespace: the trimmed removed lines and the trimmed added lines
/// say the same things in the same order.
fn whitespace_only(hunk: &DiffHunk) -> bool {
    let removed: Vec<&str> = hunk
        .lines
        .iter()
        .filter(|line| line.kind == LineKind::Removed)
        .map(|line| line.text.trim())
        .collect();
    let added: Vec<&str> = hunk
        .lines
        .iter()
        .filter(|line| line.kind == LineKind::Added)
        .map(|line| line.text.trim())
        .collect();
    !removed.is_empty() && removed == added
}

/// The flat rows for `files`, folded files cut to their heads.
fn flatten(files: &[FileDiff], folded: &HashSet<String>, hide_ws: bool) -> Vec<Row> {
    let mut rows = Vec::new();
    for (index, file) in files.iter().enumerate() {
        rows.push(Row::File { file: index });
        if folded.contains(&file.path) || file.binary {
            continue;
        }
        for (at, hunk) in file.hunks.iter().enumerate() {
            if hide_ws && whitespace_only(hunk) {
                continue;
            }
            rows.push(Row::Hunk {
                file: index,
                hunk: at,
            });
            for line in 0..hunk.lines.len() {
                rows.push(Row::Line {
                    file: index,
                    hunk: at,
                    line,
                });
            }
        }
    }
    rows
}

/// The review surface.
///
/// `node` is where the keyboard lands; the editor around the surface registers it as the
/// review's sink.
#[component]
pub fn ReviewPane(
    /// Where the keyboard lands.
    node: NodeRef,
) -> impl IntoView {
    let agent = use_agent();
    let port = NodeRef::new();

    let folded: RwSignal<HashSet<String>, LocalStorage> = RwSignal::new_local(HashSet::new());

    // The flat rows, rebuilt when the files, the folds, or the whitespace toggle move.
    let flat: RwSignal<Rc<Vec<Row>>, LocalStorage> = RwSignal::new_local(Rc::new(Vec::new()));
    {
        let agent = agent.clone();
        let flattening = zgui::reactive::RenderEffect::new(move |_| {
            let files = agent.review_files();
            let hide_ws = agent.review_ws();
            let rows = folded.with(|folded| flatten(&files, folded, hide_ws));
            flat.set(Rc::new(rows));
        });
        on_cleanup_local(move || drop(flattening));
    }
    let count = Signal::derive_local(move || flat.get().len());

    // The caret walks files; this keeps the head row of the file it is on inside the port.
    let visible = {
        let agent = agent.clone();
        zdt_view::keep_visible(
            port,
            move || {
                let at = agent.review_at();
                flat.with(|rows| {
                    rows.iter()
                        .position(|row| matches!(row, Row::File { file } if *file == at))
                        .unwrap_or(0)
                })
            },
            ROW,
        )
    };
    on_cleanup_local(move || drop(visible));

    let shown = {
        let agent = agent.clone();
        move || agent.review().is_none().then(|| "none".to_owned())
    };
    let title = {
        let agent = agent.clone();
        move || {
            agent
                .review()
                .map(|review| review.title)
                .unwrap_or_default()
        }
    };
    let tally = {
        let agent = agent.clone();
        move || {
            let files = agent.review_files();
            let (added, removed) = files.iter().fold((0, 0), |(a, r), file| {
                let (fa, fr) = file.counts();
                (a + fa, r + fr)
            });
            (files.len(), added, removed)
        }
    };
    let counts = {
        let tally = tally.clone();
        move || {
            let (files, _, _) = tally();
            if files == 0 {
                return "loading\u{2026}".to_owned();
            }
            format!("{files} file{}", if files == 1 { "" } else { "s" })
        }
    };
    let counts_added = {
        let tally = tally.clone();
        move || {
            let (files, added, _) = tally();
            if files == 0 {
                String::new()
            } else {
                format!("+{added}")
            }
        }
    };
    let counts_removed = {
        let tally = tally.clone();
        move || {
            let (files, _, removed) = tally();
            if files == 0 {
                String::new()
            } else {
                format!("\u{2212}{removed}")
            }
        }
    };
    let split_on = {
        let agent = agent.clone();
        move || agent.review_split().then(|| "true".to_owned())
    };
    let split_body = split_on.clone();
    let ws_hidden = {
        let agent = agent.clone();
        move || agent.review_ws().then(|| "true".to_owned())
    };

    let on_key = {
        let agent = agent.clone();
        move |event: &mut EventCx<'_, events::KeyDown>| {
            if agent.host().key(event, event.modifiers, crate::REGION_DIFF) {
                event.prevent_default();
                event.stop_propagation();
            }
        }
    };
    let take_focus = {
        let agent = agent.clone();
        move |_: &mut EventCx<'_, events::FocusIn>| agent.host().took_keyboard()
    };
    let close = {
        let agent = agent.clone();
        move |event: &mut EventCx<'_, events::PointerDown>| {
            event.stop_propagation();
            agent.close_review();
            agent.to_chat();
        }
    };
    let toggle_split = {
        let agent = agent.clone();
        move |event: &mut EventCx<'_, events::PointerDown>| {
            event.stop_propagation();
            agent.toggle_review_split();
        }
    };
    let toggle_ws = {
        let agent = agent.clone();
        move |event: &mut EventCx<'_, events::PointerDown>| {
            event.stop_propagation();
            agent.toggle_review_ws();
        }
    };

    let build = {
        let agent = agent.clone();
        move |index: usize| review_row(agent.clone(), folded, flat, index)
    };

    view! {
        column(class = "agent-review", style:display = shown) {
            row(class = "agent-review__head") {
                Icon(icon = icons::FILE_DIFF, class = "icon--xs")
                label(class = "agent-review__title nowrap") {{title}}
                label(class = "agent-review__counts muted nowrap") {{counts}}
                label(class = "agent-added nowrap") {{counts_added}}
                label(class = "agent-removed nowrap") {{counts_removed}}
                box(class = "fill") {}
                control(
                    class = "agent-review__toggle",
                    tabindex = Focus::Programmatic,
                    a11y:label = "Hide whitespace-only hunks",
                    attr:data-on = ws_hidden,
                    on:pointer_down = toggle_ws
                ) {
                    label {"whitespace"}
                }
                control(
                    class = "agent-review__toggle",
                    tabindex = Focus::Programmatic,
                    a11y:label = "Side by side",
                    attr:data-on = split_on,
                    on:pointer_down = toggle_split
                ) {
                    label {"split"}
                }
                control(
                    class = "agent-review__toggle",
                    tabindex = Focus::Programmatic,
                    a11y:label = "Close the review",
                    on:pointer_down = close
                ) {
                    Icon(icon = icons::X, class = "icon--xs")
                }
            }
            column(
                class = "agent-review__hold",
                node_ref = node,
                tabindex = Focus::Programmatic,
                a11y:role = Role::Document,
                a11y:label = "Changes under review",
                attr:data-split = split_body,
                on:key_down = on_key,
                on:focus_in = take_focus
            ) {
                VirtualList(
                    class = "agent-review__body",
                    node_ref = port,
                    count = count,
                    row_size = ROW,
                    label = "Diff",
                    row = move |index: usize| build(index),
                )
            }
        }
    }
}

/// The word the style sheet colours a line by.
fn kind_word(kind: LineKind) -> &'static str {
    match kind {
        LineKind::Context => "context",
        LineKind::Added => "added",
        LineKind::Removed => "removed",
    }
}

/// One virtual row. It reads the flat list and the files, so a change in either redraws it in
/// place.
fn review_row(
    agent: crate::AgentUi,
    folded: RwSignal<HashSet<String>, LocalStorage>,
    flat: RwSignal<Rc<Vec<Row>>, LocalStorage>,
    index: usize,
) -> impl IntoView + use<> {
    use zdt_view::Erase;

    view! {
        box(class = "agent-review__row") {
            {move || {
                let Some(row) = flat.with(|rows| rows.get(index).copied()) else {
                    return ().any();
                };
                let files = agent.review_files();
                match row {
                    Row::File { file } => match files.get(file) {
                        Some(diff) => file_head(&agent, folded, file, diff).any(),
                        None => ().any(),
                    },
                    Row::Hunk { file, hunk } => {
                        match files.get(file).and_then(|diff| diff.hunks.get(hunk)) {
                            Some(hunk) => view! {
                                row(class = "agent-review__hunk") {
                                    label(class = "nowrap") {{hunk.header()}}
                                }
                            }
                            .any(),
                            None => ().any(),
                        }
                    }
                    Row::Line { file, hunk, line } => match files
                        .get(file)
                        .and_then(|diff| diff.hunks.get(hunk))
                        .and_then(|hunk| hunk.lines.get(line))
                    {
                        Some(held) => line_row(&agent, &files[file].path, held).any(),
                        None => ().any(),
                    },
                }
            }}
        }
    }
}

/// One file's head: the chevron, the path, and its counts. A press moves the caret and folds.
fn file_head(
    agent: &crate::AgentUi,
    folded: RwSignal<HashSet<String>, LocalStorage>,
    index: usize,
    diff: &FileDiff,
) -> impl IntoView + use<> {
    let path = diff.path.clone();
    let (added, removed) = diff.counts();
    let caret = {
        let agent = agent.clone();
        move || (agent.review_at() == index).then(|| "true".to_owned())
    };
    let chevron = {
        let path = path.clone();
        move || {
            if folded.with(|held| held.contains(&path)) {
                icons::CHEVRON_RIGHT
            } else {
                icons::CHEVRON_DOWN
            }
        }
    };
    let (plus, minus) = if diff.binary {
        (String::new(), String::new())
    } else {
        (format!("+{added}"), format!("\u{2212}{removed}"))
    };
    let binary_shown = (!diff.binary).then(|| "none".to_owned());
    let press = {
        let (agent, path) = (agent.clone(), path.clone());
        move |event: &mut EventCx<'_, events::PointerDown>| {
            event.stop_propagation();
            agent.review_go_to(index);
            folded.update(|held| {
                if !held.remove(&path) {
                    held.insert(path.clone());
                }
            });
        }
    };
    view! {
        row(class = "agent-review__file", attr:data-caret = caret, on:pointer_down = press) {
            Icon(icon = Signal::derive_local(chevron), class = "icon--xs")
            label(class = "agent-review__path nowrap") {{path.clone()}}
            box(class = "fill") {}
            label(class = "muted nowrap", style:display = binary_shown) {"binary"}
            label(class = "agent-review__filecounts agent-added nowrap") {{plus}}
            label(class = "agent-review__filecounts agent-removed nowrap") {{minus}}
        }
    }
}

/// One line: both numbers, then the text in its syntax colours. A press opens the file there.
fn line_row(agent: &crate::AgentUi, path: &str, held: &zdt_git::Line) -> impl IntoView + use<> {
    let word = kind_word(held.kind);
    let old_text = held.old.map(|line| line.to_string()).unwrap_or_default();
    let new_text = held.new.map(|line| line.to_string()).unwrap_or_default();
    let marks = agent.review_marks();
    let body = zdt_syntax::line_view(
        &held.text,
        marks
            .get(path)
            .and_then(|sides| sides.line(held.kind, held.old, held.new))
            .map(|(spans, number)| (spans.as_ref(), number)),
    );
    let press = {
        let agent = agent.clone();
        let path = path.to_owned();
        let line = held.new.or(held.old).map(u64::from);
        move |event: &mut EventCx<'_, events::PointerDown>| {
            event.stop_propagation();
            if let Some(review) = agent.review() {
                agent.host().open_file(&review.root.join(&path), line);
            }
        }
    };
    view! {
        row(class = "agent-review__line", attr:data-kind = Some(word.to_owned()), on:pointer_down = press) {
            label(class = "agent-review__num agent-review__num--old") {{old_text}}
            label(class = "agent-review__num agent-review__num--new") {{new_text}}
            box(class = "agent-review__text") {{body}}
        }
    }
}
