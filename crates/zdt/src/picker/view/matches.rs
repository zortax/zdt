//! The matches, and one row of them.

use super::*;
use crate::picker::{Row, use_picker};
use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui::prelude::*;

/// The matches, down the left.
#[component]
pub(crate) fn Matches() -> impl IntoView {
    let picker = use_picker();

    let count = {
        let picker = picker.clone();
        move || picker.len()
    };
    let row = move |index: usize| view! { MatchRow(index = index) };

    view! {
        VirtualList(
            class = "picker__list",
            count = Signal::derive_local(count),
            row_size = ROW,
            row = row,
            label = "Matches",
        )
    }
}

/// One match.
// The provider mark is a path call the view macro wraps in braces of its own.
#[allow(unused_braces)]
#[component]
pub(crate) fn MatchRow(
    /// Where it is in the list.
    index: usize,
) -> impl IntoView {
    let picker = use_picker();

    let row = {
        let picker = picker.clone();
        move || picker.rows().get(index).cloned()
    };
    let selected = {
        let picker = picker.clone();
        move || (picker.at() == index).then(|| "true".to_owned())
    };
    let glyph = {
        let row = row.clone();
        move || row().and_then(|row| row.glyph).unwrap_or("").to_owned()
    };
    let tint = {
        let row = row.clone();
        move || {
            row()
                .and_then(|row| row.tint)
                .map(|tint| format!("var(--{tint})"))
        }
    };
    let detail = {
        let row = row.clone();
        move || row().map_or_else(String::new, |row| row.detail)
    };
    // A provider mark, in the glyph's place. Filled art, so the icon class's inherited stroke
    // is turned off for it.
    let icon = {
        let row = row.clone();
        move || row().and_then(|row| row.icon)
    };
    let icon_svg = {
        let icon = icon.clone();
        move || icon().unwrap_or(zdt_icons::DOT)
    };
    let icon_shown = move || icon().is_none().then(|| "none".to_owned());

    view! {
        row(
            class = "picker__row",
            attr:data-selected = selected,
            a11y:role = Role::ListBoxOption,
            on:pointer_down = {
                let picker = picker.clone();
                move |_| picker.go_to(index)
            },
            on:double_click = {
                let picker = picker.clone();
                move |_| {
                    picker.go_to(index);
                    picker.activate();
                }
            }
        ) {
            zdt_icons::Icon(
                icon = Signal::derive_local(icon_svg),
                class = "icon--xs icon--brand picker__icon",
                style:display = icon_shown
            )
            label(class = "glyph", style:color = tint) {{glyph}}
            row(class = "picker__label nowrap") {
                {move || {
                    let row = row().unwrap_or_else(|| Row::plain("", crate::picker::Target::Nothing));
                    highlighted(&row.label, &row.matched)
                }}
            }
            label(class = "picker__detail nowrap") {{detail}}
        }
    }
}

/// A label with the characters the query landed on picked out.
///
/// One span per run, and never one per character. A path of forty characters that matched six
/// makes seven spans this way and forty the other, and a picker redraws this on every keystroke.
fn highlighted(label: &str, matched: &[u32]) -> Vec<zgui::view::AnyView> {
    use zdt_view::Erase;

    if matched.is_empty() {
        return vec![view! { label(class = "nowrap") {{label.to_owned()}} }.any()];
    }

    let mut spans = Vec::new();
    let mut run = String::new();
    let mut hot = false;
    for (index, character) in label.chars().enumerate() {
        let lit = matched.binary_search(&(index as u32)).is_ok();
        if lit != hot && !run.is_empty() {
            spans.push(span(std::mem::take(&mut run), hot));
        }
        hot = lit;
        run.push(character);
    }
    if !run.is_empty() {
        spans.push(span(run, hot));
    }
    spans
}

/// One run of characters, lit or not.
fn span(text: String, lit: bool) -> zgui::view::AnyView {
    use zdt_view::Erase;
    let class = if lit { "picker__hit" } else { "" };
    view! { label(class = class) {{text}} }.any()
}
