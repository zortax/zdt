//! The suggestions, drawn.
//!
//! Two surfaces: the list under the caret, and — once the caret has rested on a row — what the
//! server says about that row, beside it. Both are placed by the same solver the documentation
//! panel uses, so both flip above the caret near the bottom of the window and both stay inside it.
//!
//! # Why the rows are virtualised
//!
//! `rust-analyzer` answers a bare `.` with two thousand suggestions. Twelve of them are on screen.
//! Building two thousand rows to show twelve is the difference between a popup that appears and one
//! that arrives — and it happens on every keystroke that opens one.

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::{component, view};
use zgui_ui::prelude::VirtualListProps;
use zgui_ui_primitives::popper::{Align, Placement, Side};

use crate::completion::{Item, ROW, VISIBLE, use_completion};
use crate::ui::anchor::{Anchoring, place};
use crate::ui::markdown::MarkdownProps;

/// How wide the popup is, which the style sheet also says.
///
/// Declared here as well because the documentation panel is placed against the popup's box, and on
/// the frame the popup opens there is no measured box to place against yet.
const POPUP_WIDTH: f32 = 340.0;

/// The popup and the panel beside it.
#[component]
pub fn CompletionPopup() -> impl IntoView {
    let completion = use_completion();
    let surface = NodeRef::new();
    let docs_surface = NodeRef::new();

    // Everything the popup draws, kept for the length of the exit rather than read live: closing
    // clears all of it in the same flush the presence watches, and a popup has no business
    // emptying out on the way out.
    let showing: RwSignal<Option<crate::completion::Open>, LocalStorage> =
        RwSignal::new_local(None);
    let rows: RwSignal<Vec<Item>, LocalStorage> = RwSignal::new_local(Vec::new());
    let chosen: RwSignal<usize, LocalStorage> = RwSignal::new_local(0);
    let documentation: RwSignal<Option<Vec<crate::ui::markdown::Block>>, LocalStorage> =
        RwSignal::new_local(None);
    let follow = {
        let completion = completion.clone();
        zgui::reactive::RenderEffect::new(move |_| {
            // Read first and unconditionally, so that this follows every one of them while the
            // popup is up rather than only the first that happened to be read.
            let (open, items, at, docs) = (
                completion.open(),
                completion.items(),
                completion.at(),
                completion.docs(),
            );
            if let Some(open) = open {
                showing.set(Some(open));
                rows.set(items);
                chosen.set(at);
                // The documentation comes and goes while the popup is up — it is fetched only once
                // the caret has rested on a row — so it follows freely here, and freezes with
                // everything else the moment the popup starts to leave.
                documentation.set(docs);
            }
        })
    };
    on_cleanup_local(move || drop(follow));

    let placed = place(
        surface,
        move || showing.get().map(|open| open.caret),
        Anchoring::on(Placement::new(Side::Bottom, Align::Start)),
    );

    // The panel goes beside the popup rather than beside the caret, and the solver flips it to the
    // other side when the popup is already near the window's edge. Anchored to the popup's box,
    // which is what makes "beside" mean beside the list rather than beside the character.
    let docs_placed = place(
        docs_surface,
        move || {
            let left = placed.left.get()?;
            let top = placed.top.get()?;
            Some(zgui_editor::CaretRect {
                x: left,
                y: top,
                width: POPUP_WIDTH,
                height: rows_shown(rows) * ROW,
            })
        },
        Anchoring::on(Placement::new(Side::Right, Align::Start)).offset(4.0),
    );

    let present = {
        let completion = completion.clone();
        Signal::derive_local(move || completion.open().is_some())
    };
    // Nought rows while the popup is away, so a closed popup builds none.
    let count = Signal::derive_local(move || {
        if present.get() {
            rows.with(Vec::len)
        } else {
            0
        }
    });
    // Off the kept documentation rather than the live one, so that closing the popup does not
    // clear this panel's content in the same flush that takes the panel away — the same rule as
    // the list beside it.
    let docs_present =
        Signal::derive_local(move || present.get() && documentation.with(Option::is_some));
    // Stored, because the panel is built inside a presence's own closure and that closure can
    // run more than once — a surface that comes back after leaving is built again.
    let offset = StoredValue::new_local({
        let completion = completion.clone();
        move || Some(format!("translateY({}px)", -completion.docs_offset()))
    });

    // Hidden rather than unmounted. The popup closes on any keystroke that matches nothing, which
    // is to say constantly while typing, and tearing a virtualised list out of the tree in the
    // same flush its viewport observation goes leaves its bindings reading a disposed scope.
    // Nothing is lost by keeping it: rebuilding a two-thousand-row list per keystroke was waste,
    // and the exit animation never ran anyway.
    let shown = move |placed: crate::ui::anchor::Placed, present: Signal<bool, LocalStorage>| {
        move || (!present.get() || !placed.settled.get()).then(|| "hidden".to_owned())
    };

    view! {
        column(
            class = "completion",
                node_ref = surface,
                attr:data-side = move || placed.side.get(),
                attr:data-open = move || present.get().then(|| "true".to_owned()),
                style:left = placed.left_px(),
                style:top = placed.top_px(),
                style:visibility = shown(placed, present),
                a11y:role = Role::ListBox,
                a11y:label = "Suggestions"
            ) {
                VirtualList(
                    class = "completion__list",
                    count = count,
                    row_size = ROW,
                    overscan = 4,
                    label = "Suggestions",
                    row = move |index: usize| view! {
                        Suggestion(index = index, rows = rows, chosen = chosen)
                    },
                )
        }

        // A second surface rather than a column inside the first: the panel is placed against the
        // popup and has to be able to flip to the other side of it, which a child cannot do. Kept
        // mounted for the same reason as the popup.
        column(
            class = "completion__docs",
                node_ref = docs_surface,
                attr:data-side = move || docs_placed.side.get(),
                attr:data-open = move || docs_present.get().then(|| "true".to_owned()),
                style:left = docs_placed.left_px(),
                style:top = docs_placed.top_px(),
                style:visibility = shown(docs_placed, docs_present),
                a11y:role = Role::Tooltip,
                a11y:label = "Documentation"
            ) {
                box(
                    class = "completion__docs-body",
                    style:transform = move || offset.with_value(|read| read())
                ) {
                    {move || {
                        use crate::ui::Erase;
                        match documentation.get() {
                            Some(blocks) => view! { Markdown(blocks = blocks) }.any(),
                            None => ().any(),
                        }
                    }}
                }
        }
    }
}

/// How many rows the popup is showing, capped at what fits.
fn rows_shown(rows: RwSignal<Vec<Item>, LocalStorage>) -> f32 {
    rows.with(Vec::len).min(VISIBLE) as f32
}

/// One suggestion.
///
/// Reads the popup's kept rows rather than the live state, for the same reason the popup does: a
/// row is a child of a virtualised list, and a row still reading state that is cleared as the
/// popup closes is a binding running against a scope that has just gone.
#[component]
fn Suggestion(
    /// Which row this is.
    index: usize,
    /// What the popup is showing.
    rows: RwSignal<Vec<Item>, LocalStorage>,
    /// Which row the caret is on.
    chosen: RwSignal<usize, LocalStorage>,
) -> impl IntoView {
    let item = move || rows.with(|rows| rows.get(index).cloned());
    let chosen = move || chosen.get() == index;

    let (kind, glyph, label, detail) = (item, item, item, item);
    let selected = chosen;

    view! {
        row(
            class = "completion__row",
            attr:data-selected = move || chosen().then(|| "true".to_owned()),
            a11y:role = Role::ListBoxOption,
            a11y:selected = selected
        ) {
            label(
                class = "completion__kind glyph",
                attr:data-kind = move || kind().map(|one| one.tone().to_owned())
            ) {
                {move || glyph().map(|one| one.glyph().to_owned()).unwrap_or_default()}
            }
            label(class = "completion__label nowrap") {
                {move || label().map(|one| one.label).unwrap_or_default()}
            }
            box(class = "fill") {}
            label(class = "completion__detail nowrap muted") {
                {move || detail().and_then(|one| one.detail).unwrap_or_default()}
            }
        }
    }
}
