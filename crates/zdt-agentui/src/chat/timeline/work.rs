//! One tool run or subagent, as a row.

use zdt_agent::thread::{ItemStatus, TimelineItem};
use zdt_icons::{self as icons, IconProps};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::{component, view};

use super::glyph::tool_glyph;

/// One tool or task, slim until it is asked to open.
#[component]
pub(super) fn WorkRow(
    /// The row's own signal.
    row: RwSignal<TimelineItem, LocalStorage>,
) -> impl IntoView {
    let opened: RwSignal<bool, LocalStorage> = RwSignal::new_local(false);

    let status = move || row.with(|item| item.status);
    let status_word = move || Some(status().word().to_owned());
    let glyph = move || {
        row.with(|item| match item.status {
            ItemStatus::Running => icons::CIRCLE_DASHED,
            ItemStatus::Failed => icons::CIRCLE_ALERT,
            ItemStatus::Declined => icons::CIRCLE_X,
            ItemStatus::Ok | ItemStatus::Unknown => tool_glyph(item.kind, item.tool),
        })
    };
    let name = move || row.with(|item| item.name.clone());
    let summary = move || row.with(|item| item.text.clone());
    let has_detail = move || row.with(|item| !item.detail.is_empty());
    let detail = move || {
        if opened.get() && has_detail() {
            row.with(|item| item.detail.clone())
        } else {
            String::new()
        }
    };
    let detail_shown = move || (!opened.get() || !has_detail()).then(|| "none".to_owned());

    let toggle = move |event: &mut EventCx<'_, events::PointerDown>| {
        event.stop_propagation();
        opened.update(|held| *held = !*held);
    };

    view! {
        column(class = "agent-work", attr:data-status = status_word, on:pointer_down = toggle) {
            row(class = "agent-work__head") {
                Icon(icon = Signal::derive_local(glyph), class = "icon--xs agent-work__glyph")
                label(class = "agent-work__name nowrap") {{name}}
                label(class = "agent-work__summary muted nowrap") {{summary}}
            }
            label(class = "agent-work__detail", style:display = detail_shown) {{detail}}
        }
    }
}
