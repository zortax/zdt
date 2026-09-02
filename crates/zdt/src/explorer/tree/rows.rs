//! The panel, and the rows in it.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use super::*;
use crate::explorer::drag::{Landing, landing_for};
use crate::explorer::use_explorer;
use crate::vim::use_vim;
use crate::workspace::use_workspace;
use zdt_icons::{self as icons, IconProps};
use zgui::prelude::*;
use zgui::view::time::{IntervalHandle, set_interval};
use zgui::{component, view};
use zgui_ui::prelude::*;

/// How often the list is pulled while a drag sits at one of its edges.
///
/// One frame at sixty a second. A pull that ticked more slowly would move the list in steps that
/// are visible as steps.
const PULL_TICK: Duration = Duration::from_millis(16);

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

    // ---- The pointer gesture -------------------------------------------------------------------
    //
    // On the panel and never on a row, for two reasons. A capture cuts the hit chain at the
    // capturing element, so no other row would hear the pointer once one held it — and the virtual
    // list destroys a row's element the moment it scrolls out of view, which is exactly what the
    // pull below makes happen. The panel is here for the whole gesture.
    let drag = explorer.drag();

    // Where a drop would land, from where the pointer is. Worked out rather than heard, because a
    // capture is what stops a row hearing that the pointer arrived over it.
    let landing = {
        let explorer = explorer.clone();
        move |at: zgui::geom::Point<zgui::geom::CssPx, zgui::geom::Css>| {
            let Some(held) = drag.carrying_untracked() else {
                return Landing::Nowhere;
            };
            if !viewport.holds(at) {
                return Landing::Nowhere;
            }
            let under = viewport
                .row_at(at, explorer.len())
                .and_then(|at| explorer.row_at(at));
            landing_for(under.as_ref(), &held.paths, &explorer.root())
        }
    };

    // The pointer is taken on the press, once a row has said the press could become a drag.
    //
    // Not on the move that passes the threshold, which is where a control with something clickable
    // inside it would take it. A row holds nothing to press, so there is nothing to steal a release
    // from — and waiting means a pointer that leaves the panel before it has travelled four pixels
    // is never heard from again, which is a drag that silently never starts.
    let take_pointer = move |event: &mut EventCx<'_, events::PointerDown>| {
        if drag.is_armed_untracked() {
            event.capture_pointer();
        }
    };

    let follow = {
        let landing = landing.clone();
        move |event: &mut EventCx<'_, events::PointerMove>| {
            let at = event.position;
            drag.moved(at, landing(at));
        }
    };

    // A point on the window, for the ghost to fly to.
    let corner = |rect: zdt_view::anchor::AnchorRect| {
        zgui::geom::Point::new(zgui::geom::CssPx(rect.x), zgui::geom::CssPx(rect.y))
    };
    let let_go = {
        let explorer = explorer.clone();
        let workspace = use_workspace();
        move |event: &mut EventCx<'_, events::PointerUp>| {
            if event.button == Some(PointerButton::Secondary) {
                return;
            }
            event.release_pointer();
            if drag.is_lifted_untracked() {
                // Where the ghost is let go: onto the row that receives it when there is one, and
                // back to the row it came from when there is not. Both are read before the drop,
                // which is what clears the landing.
                let source = crate::explorer::drag::home(&explorer);
                let target = match drag.landing() {
                    Landing::Into(path) => explorer
                        .index_of(&path)
                        .and_then(|at| viewport.row_rect(at))
                        .map(corner),
                    // The root is the head rather than a row, and the first row is what sits under
                    // it.
                    Landing::Root => viewport.row_rect(0).map(corner),
                    Landing::Nowhere => None,
                };
                match drag.land(
                    target.or(source).unwrap_or(event.position),
                    &explorer.root(),
                ) {
                    Some((what, into)) => {
                        crate::actions::move_all(&workspace, &explorer, what, &into);
                    }
                    None => drag.spring_back(source),
                }
                return;
            }
            // A press that never travelled. A file already opened on the way down; a directory
            // waited for this, because expanding one moves every row below the pointer and a drag
            // about to start needs that ground still.
            if let Some(at) = drag.disarm()
                && explorer.row_at(at).is_some_and(|row| row.entry.directory)
            {
                explorer.go_to(at);
                explorer.toggle_selected();
            }
        }
    };

    let taken_over = {
        let explorer = explorer.clone();
        move |event: &mut EventCx<'_, events::PointerCancel>| {
            event.release_pointer();
            drag.spring_back(crate::explorer::drag::home(&explorer));
        }
    };

    // The list follows the pointer to its own edges. A capture stops every other element hearing
    // the pointer, so nothing else can scroll while something is in flight.
    let pulling: Rc<RefCell<Option<IntervalHandle>>> = Rc::new(RefCell::new(None));
    let pulls = {
        let pulling = Rc::clone(&pulling);
        let landing = landing.clone();
        zgui::reactive::RenderEffect::new(move |_| {
            let lifted = drag.is_lifted();
            let mut held = pulling.borrow_mut();
            *held = lifted.then(|| {
                let landing = landing.clone();
                set_interval(PULL_TICK, move || {
                    let at = drag.at_untracked();
                    let pull = viewport.pull(at);
                    if pull != 0.0 {
                        viewport.nudge(pull);
                        // The rows moved under a pointer that did not, so what a drop would land on
                        // is a different row now.
                        drag.moved(at, landing(at));
                    }
                })
            });
        })
    };
    on_cleanup_local(move || drop((pulls, pulling)));

    let dragging = move || drag.is_lifted().then(|| "true".to_owned());
    let refused =
        move || (drag.is_lifted() && !drag.landing().accepts()).then(|| "true".to_owned());
    let root_drop = move || (drag.landing() == Landing::Root).then(|| "root".to_owned());

    let on_key = {
        let explorer = explorer.clone();
        move |event: &mut EventCx<'_, events::KeyDown>| {
            // A drag in progress takes Escape before anything else sees it, which is the one way
            // out of a gesture whose pointer is captured. The editor's own filter carries the same
            // rung, because a press that opened a file sends the keyboard there.
            if matches!(event.key, Key::Named(NamedKey::Escape))
                && explorer.drag().is_lifted_untracked()
            {
                explorer
                    .drag()
                    .spring_back(crate::explorer::drag::home(&explorer));
                event.prevent_default();
                event.stop_propagation();
                return;
            }
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
            attr:data-dragging = dragging,
            attr:data-refused = refused,
            style:width = {
                let explorer = explorer.clone();
                move || Some(explorer.width().px())
            },
            a11y:role = Role::Tree,
            a11y:label = "Files",
            on:key_down = on_key,
            on:focus_in = take_focus,
            on:pointer_down = take_pointer,
            on:pointer_move = follow,
            on:pointer_up = let_go,
            on:pointer_cancel = taken_over
        ) {
            // The strip above the tree lines up with the buffer line beside it, and the two
            // together are the whole width of the window's top edge. A title bar that stopped
            // where the tree began would be one a person had to aim at.
            row(
                class = "tree__head",
                attr:data-drop = root_drop,
                on:pointer_down = window.move_drag_handler()
            ) {
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
    // Where a drop would land. The band is on the *directory that receives it* — over a file, that
    // is the directory holding it, because that is where the file would go. Its contents take the
    // quieter tone, so the extent of the destination is readable without a second colour.
    let dropping = {
        let (drag, row) = (explorer.drag(), row.clone());
        move || {
            let row = row()?;
            let landing = drag.landing();
            let into = landing.path()?;
            if row.entry.path == into {
                Some("into".to_owned())
            } else if row.entry.path.starts_with(into) {
                Some("inside".to_owned())
            } else {
                None
            }
        }
    };
    // What this row is carrying, while it is in flight.
    let lifted = {
        let (drag, row) = (explorer.drag(), row.clone());
        move || drag.carries(&row()?.entry.path).then(|| "true".to_owned())
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

    // A press picks the row and arms a drag; what kind of press decides what else it means. The
    // secondary button is the menu and nothing else, the middle button is nothing at all, and a
    // modifier turns the gesture into one about the *set*.
    //
    // A file opens on the press, and not on the release, because that is the moment the gesture is
    // understood. The buffer line selects a tab on the way down for the same reason. Opening a file
    // is cheap, undoing it is one keystroke, and it moves no row — so waiting for the button to come
    // back up buys nothing and costs the impression that the tree is answering.
    //
    // A directory waits. Expanding one moves every row below the pointer, and a press that turned
    // out to be a drag would then be a drag over ground that had just shifted.
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
                return;
            }
            if event.modifiers.shift() {
                explorer.mark_through(index);
                return;
            }

            let carried = explorer.carried_from(index);
            explorer.go_to(index);
            // Where the row's own top edge is, so the ghost hangs from the row rather than jumping
            // its corner to the pointer. Asked of the viewport, whose arithmetic is already in CSS
            // pixels — the box an event carries is in device pixels.
            let top = explorer
                .viewport()
                .and_then(|viewport| viewport.row_rect(index))
                .map_or(event.position.y.0, |rect| rect.y);
            explorer.drag().arm(carried, index, event.position, top);

            if let Some(row) = explorer.row_at(index).filter(|row| !row.entry.directory) {
                crate::files::open(&workspace, row.entry.path);
            }
        }
    };

    view! {
        row(
            class = "tree__row",
            attr:data-selected = selected,
            attr:data-marked = marked,
            attr:data-cut = cut,
            attr:data-drop = dropping,
            attr:data-lifted = lifted,
            attr:data-directory = directory,
            attr:data-apart = apart,
            a11y:role = Role::TreeItem,
            on:pointer_down = press
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
