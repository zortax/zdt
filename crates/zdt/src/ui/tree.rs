//! The file tree, drawn.
//!
//! A virtualised list over the flattened tree, so a directory somebody expanded by accident costs
//! a few dozen rows rather than a hundred thousand.
//!
//! The panel stays mounted and is hidden by a style when it is closed, the way an inactive buffer
//! is. Toggling it is then a restyle rather than a rebuild, and the caret is where it was left.
//!
//! # Where its keys come from
//!
//! The same keymap as everything else, with the tree's own rows in front of it. That is what lets
//! `d` delete a file here and stay the delete operator everywhere else, while `<Leader>ff` — which
//! the tree says nothing about — still works with the keyboard in the panel.

use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui::prelude::*;

use crate::explorer::use_explorer;
use crate::icons::{self, IconProps};
use crate::vim::use_vim;

/// How tall one row is, which the list is told rather than measuring.
const ROW: f32 = 22.0;

/// The panel.
#[component]
pub fn Explorer() -> impl IntoView {
    let explorer = use_explorer();
    let vim = use_vim();
    let node = NodeRef::new();

    // The keyboard follows the panel: taking it is what makes `j` walk the tree rather than the
    // buffer, and giving it back is what makes `<Esc>` return to the editor.
    let claiming = {
        let explorer = explorer.clone();
        let timers = zgui::view::time::Timers::current();
        let held: std::cell::RefCell<Option<zgui::view::time::TimeoutHandle>> =
            std::cell::RefCell::new(None);
        zgui::reactive::RenderEffect::new(move |_| {
            // Read first, so that asking again for a keyboard the panel believes it already has
            // still runs this: what the prompt borrowed has to be reclaimable.
            let claims = explorer.claims();
            if !explorer.is_open() || !explorer.is_focused() || claims == 0 {
                return;
            }
            let Some(timers) = timers.as_ref() else {
                return;
            };
            // From a timer, because the first run happens while the panel is still being built and
            // a node that is not mounted cannot take focus.
            *held.borrow_mut() =
                Some(timers.set_timeout(std::time::Duration::ZERO, move || node.focus()));
        })
    };
    on_cleanup_local(move || drop(claiming));

    let open = {
        let explorer = explorer.clone();
        move || explorer.is_open().then(|| "true".to_owned())
    };
    let focused = {
        let explorer = explorer.clone();
        move || explorer.is_focused().then(|| "true".to_owned())
    };
    let root_name = {
        let explorer = explorer.clone();
        move || {
            explorer
                .root()
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        }
    };
    let count = {
        let explorer = explorer.clone();
        move || explorer.len()
    };
    let row = move |index: usize| view! { TreeRow(index = index) };
    let take_focus = {
        let explorer = explorer.clone();
        move |_: &mut EventCx<'_, events::FocusIn>| explorer.focus()
    };

    let on_key = move |event: &mut EventCx<'_, events::KeyDown>| {
        let Some(chord) = crate::keys::chord_of(event, event.modifiers) else {
            return;
        };
        if vim.key_in_region(chord, "tree") {
            event.prevent_default();
            event.stop_propagation();
        }
    };

    view! {
        column(
            class = "tree",
            node_ref = node,
            tabindex = Focus::Programmatic,
            attr:data-open = open,
            attr:data-focused = focused,
            a11y:role = Role::Tree,
            a11y:label = "Files",
            on:key_down = on_key,
            on:focus_in = take_focus
        ) {
            row(class = "tree__head") {
                Icon(icon = icons::LIST_TREE, class = "icon--sm")
                label(class = "tree__root nowrap") {{root_name}}
            }
            VirtualList(
                class = "tree__rows",
                count = Signal::derive_local(count),
                row_size = ROW,
                row = row,
                label = "Files",
            )
        }
    }
}

/// One file or directory.
///
/// Everything it draws is read from the explorer inside a tracked closure rather than handed in.
/// A row is built once and kept: what it was told at construction is what it would show for ever,
/// so the caret would never appear to move and an expanded directory would keep its closed glyph.
#[component]
fn TreeRow(
    /// Where it is in the list.
    index: usize,
) -> impl IntoView {
    let explorer = use_explorer();

    let row = {
        let explorer = explorer.clone();
        move || explorer.rows().get(index).cloned()
    };
    let selected = {
        let explorer = explorer.clone();
        move || (explorer.at() == index).then(|| "true".to_owned())
    };
    let directory = {
        let row = row.clone();
        move || {
            row()
                .filter(|row| row.entry.directory)
                .map(|_| "true".to_owned())
        }
    };
    // Twelve pixels a level: enough to read the shape, narrow enough that a deep tree still fits
    // in a panel this width.
    let indent = {
        let row = row.clone();
        move || Some(format!("{}px", 8 + row().map_or(0, |row| row.depth) * 12))
    };
    let glyph = {
        let row = row.clone();
        move || {
            row().map_or_else(String::new, |row| {
                if row.entry.directory && row.expanded {
                    zdt_core::language::DIRECTORY_OPEN.glyph
                } else {
                    row.entry.file_type().glyph
                }
                .to_owned()
            })
        }
    };
    let tint = {
        let row = row.clone();
        move || row().map(|row| format!("var(--{})", row.entry.file_type().tint))
    };
    let name = {
        let row = row.clone();
        move || row().map_or_else(String::new, |row| row.entry.name)
    };

    view! {
        row(
            class = "tree__row",
            attr:data-selected = selected,
            attr:data-directory = directory,
            style:padding-left = indent,
            a11y:role = Role::TreeItem,
            on:pointer_down = move |_| {
                explorer.go_to(index);
                explorer.focus();
            }
        ) {
            label(class = "glyph", style:color = tint) {{glyph}}
            label(class = "tree__name nowrap") {{name}}
        }
    }
}
