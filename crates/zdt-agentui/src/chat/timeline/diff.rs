//! What one turn changed, as a card.

use zdt_agent::change::{FileStat, TurnDiff};
use zdt_agent::thread::TimelineItem;
use zdt_icons::{self as icons, IconProps};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::{component, view};

use crate::use_agent;

/// What one turn changed: a one-line card, the files a press away.
///
/// The head says how much moved. Open, it lists every file with its counts; a press on a file
/// opens it in the editor. Two quiet controls review the whole span line by line or put the turn
/// back.
// The list macro takes a closure by construction, so the one it is handed here is not redundant.
#[allow(clippy::redundant_closure)]
#[component]
pub(super) fn DiffRow(
    /// The row's own signal.
    row: RwSignal<TimelineItem, LocalStorage>,
) -> impl IntoView {
    let agent = use_agent();
    let opened: RwSignal<bool, LocalStorage> = RwSignal::new_local(false);

    let diff = move || row.with(|item| TurnDiff::decode(&item.detail).unwrap_or_default());
    let files = move || diff().files;
    // The head's pieces, split so the counts can wear the diff's colours.
    let word = move || {
        let count = files().len();
        format!("{count} file{}", if count == 1 { "" } else { "s" })
    };
    let added = move || {
        let total: u32 = files().iter().map(|file| file.added).sum();
        format!("+{total}")
    };
    let removed = move || {
        let total: u32 = files().iter().map(|file| file.removed).sum();
        format!("\u{2212}{total}")
    };
    let files_shown = move || (!opened.get()).then(|| "none".to_owned());
    let chevron = move || {
        if opened.get() {
            icons::CHEVRON_DOWN
        } else {
            icons::CHEVRON_RIGHT
        }
    };

    let toggle = move |event: &mut EventCx<'_, events::PointerDown>| {
        event.stop_propagation();
        opened.update(|held| *held = !*held);
    };
    let review = {
        let agent = agent.clone();
        move |event: &mut EventCx<'_, events::PointerDown>| {
            event.stop_propagation();
            agent.review_turn(&diff());
        }
    };
    let revert = {
        let agent = agent.clone();
        move |event: &mut EventCx<'_, events::PointerDown>| {
            event.stop_propagation();
            agent.revert_turn(diff().turn);
        }
    };

    let open_file = {
        let agent = agent.clone();
        move |path: &str| {
            if let Some(shell) = agent.selected_shell() {
                agent.host().open_file(&shell.root.join(path), None);
            }
        }
    };

    view! {
        column(class = "agent-diffcard") {
            row(class = "agent-diffcard__head", on:pointer_down = toggle) {
                Icon(icon = icons::FILE_DIFF, class = "icon--xs agent-diffcard__glyph")
                label(class = "agent-diffcard__word") {{word}}
                label(class = "agent-added nowrap") {{added}}
                label(class = "agent-removed nowrap") {{removed}}
                box(class = "fill") {}
                control(
                    class = "agent-diffcard__act",
                    tabindex = Focus::Programmatic,
                    a11y:label = "Review the changes",
                    on:pointer_down = review
                ) {
                    Icon(icon = icons::EYE, class = "icon--xs")
                    label {"review"}
                }
                control(
                    class = "agent-diffcard__act",
                    tabindex = Focus::Programmatic,
                    a11y:label = "Revert the turn",
                    on:pointer_down = revert
                ) {
                    Icon(icon = icons::HISTORY, class = "icon--xs")
                    label {"revert"}
                }
                Icon(
                    icon = Signal::derive_local(chevron),
                    class = "icon--xs agent-diffcard__chevron"
                )
            }
            column(class = "agent-diffcard__files", style:display = files_shown) {
                for file in move || files(), key = |file: &FileStat| file.path.clone() {
                    {diff_file_row(&file, open_file.clone())}
                }
            }
        }
    }
}

/// One file of a diff card: the path, its counts, and a press that opens it.
fn diff_file_row<F: Fn(&str) + Clone + 'static>(
    file: &FileStat,
    open_file: F,
) -> impl IntoView + use<F> {
    let path = file.path.clone();
    let (added, removed) = if file.binary {
        (String::new(), String::new())
    } else {
        (
            format!("+{}", file.added),
            format!("\u{2212}{}", file.removed),
        )
    };
    let binary_shown = (!file.binary).then(|| "none".to_owned());
    let open = {
        let path = path.clone();
        move |event: &mut EventCx<'_, events::PointerDown>| {
            event.stop_propagation();
            open_file(&path);
        }
    };
    view! {
        row(class = "agent-diffcard__file", on:pointer_down = open) {
            label(class = "agent-diffcard__path nowrap") {{path.clone()}}
            box(class = "fill") {}
            label(class = "muted nowrap", style:display = binary_shown) {"binary"}
            label(class = "agent-added nowrap") {{added}}
            label(class = "agent-removed nowrap") {{removed}}
        }
    }
}
