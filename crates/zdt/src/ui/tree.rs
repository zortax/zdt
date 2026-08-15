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
use crate::workspace::use_workspace;

/// How tall one row is, which the list is told rather than measuring.
const ROW: f32 = 22.0;

/// The panel.
#[component]
pub fn Explorer() -> impl IntoView {
    let explorer = use_explorer();
    let vim = use_vim();
    let window = use_window();
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
            // The strip above the tree lines up with the buffer line beside it, and the two
            // together are the whole width of the window's top edge. A title bar that stopped
            // where the tree began would be one a person had to aim at.
            row(class = "tree__head", on:pointer_down = window.move_drag_handler()) {
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
    let marked = {
        let (explorer, row) = (explorer.clone(), row.clone());
        move || {
            let row = row()?;
            explorer
                .is_marked(&row.entry.path)
                .then(|| "true".to_owned())
        }
    };
    let cut = {
        let (explorer, row) = (explorer.clone(), row.clone());
        move || {
            let row = row()?;
            let held = explorer.clipboard()?;
            (held.cut && held.path == row.entry.path).then(|| "true".to_owned())
        }
    };
    let dropping = {
        let (explorer, row) = (explorer.clone(), row.clone());
        move || {
            let row = row()?;
            (explorer.drop_target()? == row.entry.path).then(|| "into".to_owned())
        }
    };
    let is_directory = {
        let row = row.clone();
        move || row().is_some_and(|row| row.entry.directory)
    };
    let directory = {
        let is_directory = is_directory.clone();
        move || is_directory().then(|| "true".to_owned())
    };
    let depth = {
        let row = row.clone();
        move || row().map_or(0, |row| row.depth)
    };
    // Whether the disclosure chevron is turned down. Written as an attribute rather than as two
    // different icons, because a chevron that *turns* is the whole of what says a directory
    // opened: the rows below it are recycled by the virtual list rather than created, so nothing
    // among them can carry an animation, and the folder glyph swaps one character for another
    // with nothing in between to interpolate.
    let expanded = {
        let row = row.clone();
        move || {
            row()
                .filter(|row| row.entry.directory && row.expanded)
                .map(|_| "true".to_owned())
        }
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

    // A press picks the row, and a plain one opens it. What kind of press decides which: the
    // secondary button is the menu and nothing else, the middle button is nothing at all, and a
    // modifier turns the gesture into one about the *set* rather than about this row.
    //
    // On the press rather than the release, because that is the moment the gesture is understood
    // — the same reason the buffer line selects a tab on the way down. Opening a file is cheap and
    // undoing it is one keystroke, so waiting for the button to come back up buys nothing and
    // costs the impression that the tree is answering.
    let press = {
        let explorer = explorer.clone();
        let workspace = use_workspace();
        move |event: &mut EventCx<'_, events::PointerDown>| {
            explorer.focus();
            match event.button {
                Some(PointerButton::Secondary) => {
                    // A right click on a row nobody has picked out acts on that row; on one of
                    // several, it acts on all of them. It opens the menu and *only* the menu: a
                    // press that both asked what to do and did something would be one that
                    // answered its own question.
                    if !explorer
                        .row_at(index)
                        .is_some_and(|row| explorer.is_marked(&row.entry.path))
                    {
                        explorer.clear_marks();
                        explorer.go_to(index);
                    }
                    crate::ui::treemenu::open_at(event.position);
                    return;
                }
                Some(PointerButton::Middle) => return,
                _ => {}
            }

            if event.modifiers.control() {
                explorer.toggle_mark(index);
            } else if event.modifiers.shift() {
                explorer.mark_through(index);
            } else {
                explorer.clear_marks();
                explorer.go_to(index);
                explorer.start_drag(index);
                // Both ways, the way the keyboard's `<CR>` is: it opens a closed directory and
                // closes an open one.
                if let Some(path) = explorer.toggle_selected() {
                    crate::files::open(&workspace, path);
                }
            }
        }
    };

    // What the release is left with is the drop: a press that travelled to another row is a move,
    // and one that went nowhere has already done everything it was going to do.
    let release = {
        let explorer = explorer.clone();
        let workspace = use_workspace();
        move |event: &mut EventCx<'_, events::PointerUp>| {
            if event.button == Some(PointerButton::Secondary) {
                return;
            }
            if let Some((from, into)) = explorer.finish_drag() {
                crate::actions::move_into(&workspace, &explorer, &from, &into);
            }
        }
    };

    let over = {
        let explorer = explorer.clone();
        move |_: &mut EventCx<'_, events::PointerEnter>| explorer.drag_over(index)
    };

    view! {
        row(
            class = "tree__row",
            attr:data-selected = selected,
            attr:data-marked = marked,
            attr:data-cut = cut,
            attr:data-drop = dropping,
            attr:data-directory = directory,
            a11y:role = Role::TreeItem,
            on:pointer_down = press,
            on:pointer_up = release,
            on:pointer_enter = over
        ) {
            // One rail per level of nesting, each drawing the line that traces back to the
            // directory it belongs to.
            {move || {
                (0..depth())
                    .map(|_| view! { box(class = "tree__rail") { box(class = "tree__rail__line") {} } })
                    .collect::<Vec<_>>()
            }}
            box(class = "tree__twist", attr:data-open = expanded) {
                {move || {
                    use crate::ui::Erase;
                    if is_directory() {
                        view! { Icon(icon = icons::CHEVRON_RIGHT, class = "icon--xs") }.any()
                    } else {
                        ().any()
                    }
                }}
            }
            label(class = "glyph", style:color = tint) {{glyph}}
            label(class = "tree__name nowrap") {{name}}
        }
    }
}
