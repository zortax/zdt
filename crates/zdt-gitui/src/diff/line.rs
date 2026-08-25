//! One row of the diff: a file's heading, a hunk's header, or a line.

use zgui::prelude::*;
use zgui::{component, view};

use crate::use_gitui;
use zdt_view::Erase;

use crate::panel::List;

use crate::diff::DiffRow;

/// One row of the diff: a file's heading, a hunk's header, or a line.
#[component]
pub(crate) fn DiffLine(
    /// Where it is in the flattened diff.
    index: usize,
) -> impl IntoView {
    let git = use_gitui();

    let found = {
        let git = git.clone();
        move || git.diff_rows().get(index).cloned()
    };
    // Whether the caret is in the hunk this row belongs to, which is what `s` would stage. The
    // mark goes down the leading edge. Tints across a diff already mean added and removed, and a
    // third tint would be a third thing to learn.
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
            git.host().took_keyboard();
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
                        file,
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
                        let marks = git.diff_marks();
                        let coloured = marks
                            .get(file)
                            .and_then(|sides| sides.line(kind, old, new))
                            .map(|(held, number)| (held.as_ref(), number));
                        let body = zdt_syntax::line_view(&text, coloured);
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
                                box(class = "git__line-text") {{body}}
                            }
                        }
                        .any()
                    }
                }
            }}
        }
    }
}
