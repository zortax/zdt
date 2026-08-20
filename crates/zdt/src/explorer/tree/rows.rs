//! The panel, and the rows in it.

use super::*;
use crate::explorer::use_explorer;
use crate::vim::use_vim;
use crate::workspace::use_workspace;
use zdt_icons::{self as icons, IconProps};
use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui::prelude::*;

/// The panel.
#[component]
pub fn Explorer() -> impl IntoView {
    let explorer = use_explorer();
    let vim = use_vim();
    let window = use_window();
    let node = NodeRef::new();

    // How the keyboard reaches the panel. Taking it is what makes `j` walk the tree, and the model
    // saying the panes have it again is what makes `<Esc>` return to the editor.
    crate::focus::claim::sink(crate::focus::Spot::Tree, crate::focus::Sink::Node(node));

    // What the list can see, which is how far a half page is, which rows a leap may label, and
    // where a row is for a field to open under.
    let viewport = Viewport::new(NodeRef::new());
    explorer.set_viewport(viewport);

    // The caret stays on screen. `j` at the bottom edge scrolls, as it does in any list, and
    // `<C-d>` would otherwise move a caret nobody could see.
    let following = {
        let explorer = explorer.clone();
        zgui::reactive::RenderEffect::new(move |_| viewport.keep_in_view(explorer.at()))
    };
    on_cleanup_local(move || drop(following));

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

    let on_key = {
        let explorer = explorer.clone();
        move |event: &mut EventCx<'_, events::KeyDown>| {
            let Some(chord) = crate::keys::chord_of(event, event.modifiers) else {
                return;
            };
            // A leap in progress takes every key, before the keymap sees one: once it has started,
            // each key is a character it aims at or a label, and a binding that answered any of
            // them would put some letters out of reach.
            if crate::explorer::leap::key(&vim, &explorer, chord)
                || vim.key_in_region(chord, "tree")
            {
                event.prevent_default();
                event.stop_propagation();
            }
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
                node_ref = viewport.node(),
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
/// Everything it draws is read from the explorer inside a tracked closure. A row is built once
/// and kept, so what it was told at construction is what it would show for ever. The caret would
/// never appear to move, and an expanded directory would keep its closed glyph.
#[component]
pub(crate) fn TreeRow(
    /// Where it is in the list.
    index: usize,
) -> impl IntoView {
    let explorer = use_explorer();
    let vim = use_vim();

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
    // Whether the disclosure chevron is turned down. Written as an attribute, and not as two
    // different icons, because a chevron that *turns* is the whole of what says a directory
    // opened. The virtual list recycles the rows below it, so nothing among them can carry an
    // animation, and the folder glyph swaps one character for another with nothing in between to
    // interpolate.
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
    // A dotfile, or something the ignore files leave out, shown because somebody asked for it.
    // Drawn faintly, so the eye can tell what belongs to the project from what is merely in the
    // directory.
    let apart = {
        let row = row.clone();
        move || {
            row()
                .filter(|row| row.entry.standing.is_apart())
                .map(|_| "true".to_owned())
        }
    };
    // What git says about it, as one outline at the trailing edge. The name itself keeps its own
    // colour: a tree that tints filenames by their git state has thrown away what the tint of a
    // filename is for.
    let mark = {
        let (row, status) = (row.clone(), crate::git::use_status());
        move || row().and_then(|row| status.mark(&row.entry.path))
    };
    // The key that leaps to this row, while one is being chosen.
    let leap = {
        let leaping = vim.leaping();
        move || leaping.label_at(index).map(|key| key.to_string())
    };

    // A press picks the row, and a plain one opens it. What kind of press decides which. The
    // secondary button is the menu and nothing else, the middle button is nothing at all, and a
    // modifier turns the gesture into one about the *set*.
    //
    // On the press, and not the release, because that is the moment the gesture is understood. The
    // buffer line selects a tab on the way down for the same reason. Opening a file is cheap and
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
                    crate::explorer::menu::open_at(event.position);
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
            attr:data-apart = apart,
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
                    use zdt_view::Erase;
                    if is_directory() {
                        view! { Icon(icon = icons::CHEVRON_RIGHT, class = "icon--xs") }.any()
                    } else {
                        ().any()
                    }
                }}
            }
            label(class = "glyph", style:color = tint) {{glyph}}
            label(class = "tree__name nowrap") {{name}}
            // Pushed to the trailing edge by the name's own growth, so neither of these covers a
            // filename that fits.
            {move || {
                use zdt_view::Erase;
                match mark() {
                    Some(mark) => view! {
                        box(
                            class = "tree__git",
                            style:color = Some(format!("var(--{})", mark.tint()))
                        ) {
                            Icon(icon = mark.icon(), class = "icon--xs")
                        }
                    }
                    .any(),
                    None => ().any(),
                }
            }}
            // One element, always here, hidden by an attribute: a leap must create and destroy no
            // nodes among rows the list is recycling.
            label(
                class = "tree__leap",
                attr:data-on = {
                    let leap = leap.clone();
                    move || leap().is_some().then(|| "true".to_owned())
                }
            ) {{move || leap().unwrap_or_default()}}
        }
    }
}
