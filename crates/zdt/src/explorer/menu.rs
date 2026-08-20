//! The tree's context menu.
//!
//! What a right click opens. Every row in it runs the same action a key does, so the menu can
//! never drift from the keymap. It is a second way to reach `tree.create`, and one implementation
//! of creating a file.
//!
//! Placed where the pointer was. That is where the hand already is, and a row is 22 pixels tall,
//! so a menu anchored to the row would cover the thing it is about.

use zdt_icons::{self as icons, IconProps};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::{component, view};
use zgui_ui_primitives::prelude::*;

use crate::explorer::use_explorer;
use crate::vim::use_vim;

/// Where the menu is, when it is open.
///
/// A signal, and no context object. There is one tree and one menu, and a type whose whole
/// content is a pair of numbers earns nothing.
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

/// Where a menu-started action should open a field.
///
/// A cell and not the position signal, because closing the menu is what runs the action and the
/// position is gone by then.
fn handed() -> &'static std::thread::LocalKey<std::cell::Cell<Option<(f32, f32)>>> {
    thread_local! {
        static HANDED: std::cell::Cell<Option<(f32, f32)>> = const { std::cell::Cell::new(None) };
    }
    &HANDED
}

/// Says the action about to run came from the menu, and where the menu was opened.
pub fn hand_over() {
    let at = position().get_untracked();
    handed().with(|held| held.set(at));
}

/// Where a menu-started action should open a field, taken once.
///
/// Taken, so a field started from the keyboard afterwards opens on the caret row rather than where
/// a menu last was.
#[must_use]
pub fn taken_at() -> Option<(f32, f32)> {
    handed().with(std::cell::Cell::take)
}

/// One row of the menu: an outline, what it says, and the action it runs.
struct Item {
    /// The outline beside it.
    icon: &'static str,
    /// What it says.
    label: &'static str,
    /// The action it runs.
    action: &'static str,
    /// A second argument, for the actions that take one.
    flag: Option<&'static str>,
}

impl Item {
    /// The action this row runs, as a keymap writes one.
    ///
    /// Built rather than held, so the row and the keymap can be compared: the arguments are part of
    /// what tells "New file" from "New directory".
    fn action(&self) -> zdt_vim::Action {
        let mut table = toml::Table::new();
        if let Some(flag) = self.flag {
            table.insert(flag.to_owned(), toml::Value::Boolean(true));
        }
        zdt_vim::Action {
            name: self.action.to_owned(),
            args: zdt_vim::Args::new(table),
        }
    }
}

/// A run of rows, with a rule above it.
///
/// A slice of runs, and no separator among the rows: a rule at either end is then a thing that
/// cannot be written down.
struct Group(&'static [Item]);

/// What the menu offers, in the order it offers it.
///
/// Grouped by what kind of act each is. Ten rows in one column is a wall, and the four kinds are
/// four different questions somebody opened the menu to ask.
const GROUPS: &[Group] = &[
    Group(&[Item {
        icon: icons::ARROW_RIGHT,
        label: "Open",
        action: "tree.child_or_open",
        flag: None,
    }]),
    Group(&[
        Item {
            icon: icons::FILE_PLUS,
            label: "New file",
            action: "tree.create",
            flag: None,
        },
        Item {
            icon: icons::FOLDER_PLUS,
            label: "New directory",
            action: "tree.create",
            flag: Some("directory"),
        },
    ]),
    Group(&[
        Item {
            icon: icons::PENCIL,
            label: "Rename",
            action: "tree.rename",
            flag: None,
        },
        Item {
            icon: icons::REPLACE,
            label: "Move",
            action: "tree.move",
            flag: None,
        },
        Item {
            icon: icons::TRASH,
            label: "Delete",
            action: "tree.delete",
            flag: None,
        },
    ]),
    Group(&[
        Item {
            icon: icons::COPY,
            label: "Copy",
            action: "tree.copy",
            flag: None,
        },
        Item {
            icon: icons::SCISSORS,
            label: "Cut",
            action: "tree.cut",
            flag: None,
        },
        Item {
            icon: icons::CLIPBOARD_PASTE,
            label: "Paste",
            action: "tree.paste",
            flag: None,
        },
    ]),
    Group(&[
        Item {
            icon: icons::PLUS,
            label: "Stage",
            action: "tree.stage",
            flag: None,
        },
        Item {
            icon: icons::MINUS,
            label: "Unstage",
            action: "tree.unstage",
            flag: None,
        },
        Item {
            icon: icons::EYE_OFF,
            label: "Ignore",
            action: "tree.ignore",
            flag: None,
        },
    ]),
    Group(&[
        Item {
            icon: icons::CLIPBOARD_COPY,
            label: "Copy path",
            action: "tree.copy_path",
            flag: None,
        },
        Item {
            icon: icons::EXTERNAL_LINK,
            label: "Reveal in desktop",
            action: "tree.system_open",
            flag: None,
        },
    ]),
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
                use zdt_view::Erase;
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

    let rows = GROUPS
        .iter()
        .enumerate()
        .flat_map(|(group, Group(items))| {
            use zdt_view::Erase;

            // A rule above every group but the first. The first has the menu's own edge above it.
            //
            // Hidden from a reader: a rule is the eye's way of grouping rows, and a reader walks
            // the rows themselves.
            let rule =
                (group > 0).then(|| view! { box(class = "treemenu__rule", a11y:hidden = true) {} });

            let rows = items.iter().map(|item| {
                let action = item.action();
                // The key that runs it, read from the tree's own rows so the menu can never drift
                // from the keymap. The revision is read beside it, so a keymap loaded again
                // redraws the column.
                let keys = {
                    let (keymaps, action) = (vim.keymaps().clone(), item.action());
                    Signal::derive_local(move || {
                        let _ = keymaps.revision();
                        keymaps
                            .keys_for(Some("tree"), zdt_vim::Mode::Normal, &action)
                            .first()
                            .cloned()
                            .unwrap_or_default()
                    })
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
                            // Where the menu was, for whatever the row opens beside. Taken before
                            // the close, which is what clears the position.
                            hand_over();
                            close();
                            // The tree keeps the keyboard, so whatever the row started carries on
                            // as though a key had asked for it. That covers a field and a paste.
                            explorer.focus();
                            vim.run(&action);
                        }
                    ) {
                        Icon(icon = item.icon, class = "icon--xs")
                        label(class = "treemenu__label nowrap") {{item.label}}
                        label(class = "treemenu__key") {{move || keys.get()}}
                    }
                }
            });

            std::iter::once(rule.any()).chain(rows.map(Erase::any))
        })
        .collect::<Vec<_>>();

    view! {
        // A press anywhere else closes it, which is what every menu everywhere does.
        box(
            class = "treemenu__scrim",
            attr:data-state = move || zdt_view::leaving_state(leaving),
            on:pointer_down = move |_| close()
        ) {}
        column(
            class = "treemenu",
            node_ref = surface,
            attr:data-state = move || zdt_view::leaving_state(leaving),
            style:left = Some(format!("{x}px")),
            style:top = Some(format!("{y}px")),
            a11y:role = Role::Menu,
            a11y:label = "File"
        ) {
            {rows}
        }
    }
}
