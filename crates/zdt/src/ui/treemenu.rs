//! The tree's context menu.
//!
//! What a right click opens. Every row in it runs the same action a key does, so the menu can
//! never drift from the keymap: it is a second way to reach `tree.create`, not a second
//! implementation of creating a file.
//!
//! Placed where the pointer was rather than anchored to the row, because that is where the hand
//! already is and because a row is 22 pixels tall — an anchored menu would cover the thing it is
//! about.

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::{component, view};
use zgui_ui_primitives::prelude::*;

use crate::explorer::use_explorer;
use crate::vim::use_vim;

/// Where the menu is, when it is open.
///
/// A signal rather than a context object: there is one tree and one menu, and the alternative is
/// a type whose whole content is a pair of numbers.
fn position() -> RwSignal<Option<(f32, f32)>, LocalStorage> {
    zgui::reactive::use_local_context::<MenuAt>()
        .map(|held| held.0)
        .unwrap_or_else(|| RwSignal::new_local(None))
}

/// The signal, in context so every part of the interface reads the same one.
#[derive(Clone, Copy)]
pub struct MenuAt(pub RwSignal<Option<(f32, f32)>, LocalStorage>);

/// Puts it where every component can find it.
pub fn provide() {
    zgui::reactive::provide_local_context(MenuAt(RwSignal::new_local(None)));
}

/// Opens the menu at a pointer position.
pub fn open_at(at: zgui::geom::Point<zgui::geom::CssPx, zgui::geom::Css>) {
    position().set(Some((at.x.0, at.y.0)));
}

/// Closes it.
pub fn close() {
    let at = position();
    if at.with_untracked(Option::is_some) {
        at.set(None);
    }
}

/// One row of the menu: what it says, and the action it runs.
struct Item {
    label: &'static str,
    action: &'static str,
    /// A second argument, for the actions that take one.
    flag: Option<&'static str>,
}

/// What the menu offers, in the order it offers it.
const ITEMS: &[Item] = &[
    Item {
        label: "Open",
        action: "tree.child_or_open",
        flag: None,
    },
    Item {
        label: "New file",
        action: "tree.create",
        flag: None,
    },
    Item {
        label: "New directory",
        action: "tree.create",
        flag: Some("directory"),
    },
    Item {
        label: "Rename",
        action: "tree.rename",
        flag: None,
    },
    Item {
        label: "Copy",
        action: "tree.copy",
        flag: None,
    },
    Item {
        label: "Cut",
        action: "tree.cut",
        flag: None,
    },
    Item {
        label: "Paste",
        action: "tree.paste",
        flag: None,
    },
    Item {
        label: "Copy path",
        action: "tree.copy_path",
        flag: None,
    },
    Item {
        label: "Delete",
        action: "tree.delete",
        flag: None,
    },
    Item {
        label: "Reveal in desktop",
        action: "tree.system_open",
        flag: None,
    },
];

/// The menu.
#[component]
pub fn TreeMenu() -> impl IntoView {
    let at = position();
    let surface = NodeRef::new();

    // Where it was opened, kept for the length of the exit: the position is cleared the moment the
    // menu closes, and a menu that read it directly would jump to the corner as it left.
    let showing: RwSignal<Option<(f32, f32)>, LocalStorage> = RwSignal::new_local(None);
    let follow = zgui::reactive::RenderEffect::new(move |_| {
        if let Some(place) = at.get() {
            showing.set(Some(place));
        }
    });
    on_cleanup_local(move || drop(follow));

    view! {
        Presence(present = Signal::derive_local(move || at.get().is_some()), surface = surface) {
            {move || {
                use crate::ui::Erase;
                match showing.get() {
                    Some((x, y)) => view! { Menu(x = x, y = y, surface = surface) }.any(),
                    None => ().any(),
                }
            }}
        }
    }
}

/// One open menu.
#[component]
fn Menu(
    /// Where the pointer was.
    x: f32,
    /// The same.
    y: f32,
    /// The menu itself, whose exit animation says when it may be taken away.
    surface: NodeRef,
) -> impl IntoView {
    let leaving = use_presence();
    let vim = use_vim();
    let explorer = use_explorer();

    let rows = ITEMS
        .iter()
        .map(|item| {
            let mut table = toml::Table::new();
            if let Some(flag) = item.flag {
                table.insert(flag.to_owned(), toml::Value::Boolean(true));
            }
            let args = zdt_vim::Args::new(table);
            let action = zdt_vim::Action {
                name: item.action.to_owned(),
                args,
            };
            let vim = vim.clone();
            let explorer = explorer.clone();
            view! {
                control(
                    class = "treemenu__item",
                    tabindex = Focus::Programmatic,
                    a11y:label = item.label,
                    on:pointer_down = move |event: &mut EventCx<'_, events::PointerDown>| {
                        event.stop_propagation();
                        close();
                        // The tree keeps the keyboard, so whatever the row started — a prompt, a
                        // paste — carries on as though a key had asked for it.
                        explorer.focus();
                        vim.run(&action);
                    }
                ) {
                    label(class = "nowrap") {{item.label}}
                }
            }
        })
        .collect::<Vec<_>>();

    view! {
        // A press anywhere else closes it, which is what every menu everywhere does.
        box(
            class = "treemenu__scrim",
            attr:data-state = move || crate::ui::leaving_state(leaving),
            on:pointer_down = move |_| close()
        ) {}
        column(
            class = "treemenu",
            node_ref = surface,
            attr:data-state = move || crate::ui::leaving_state(leaving),
            style:left = Some(format!("{x}px")),
            style:top = Some(format!("{y}px")),
            a11y:role = Role::Menu,
            a11y:label = "File"
        ) {
            {rows}
        }
    }
}
