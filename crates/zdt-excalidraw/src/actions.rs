//! Everything the keys can do.
//!
//! One function per key, so the keymap names an action and this decides what it means. A leaf the
//! editor has no word for does nothing, and the key falls through to whatever is layered under the
//! editor's own map.

use excalidraw::scene::{Change, Order};
use excalidraw::{Command, Id};
use kurbo::Vec2;
use zgui::reactive::prelude::*;

use crate::state::{Board, Tool};

/// Which region's keys the editor answers in.
pub const REGION: &str = "excalidraw";

/// The keys it ships with.
pub const KEYMAP: &str = include_str!("../assets/keymap.toml");

/// The style sheet it draws with.
pub const STYLE: &str = include_str!("../assets/style.css");

/// Does what `leaf` names on `board`.
///
/// Answers whether the drawing changed, which is what tells a host to write.
pub fn run(board: &Board, leaf: &str) -> bool {
    let selected = || -> Vec<Id> { board.read_untracked().selection().to_vec() };

    match leaf {
        // --- the tools ---------------------------------------------------------------------
        "select" => tool(board, Tool::Select),
        "hand" => tool(board, Tool::Hand),
        "rectangle" => tool(board, Tool::Rectangle),
        "diamond" => tool(board, Tool::Diamond),
        "ellipse" => tool(board, Tool::Ellipse),
        "arrow" => tool(board, Tool::Arrow),
        "line" => tool(board, Tool::Line),
        "freedraw" => tool(board, Tool::Freedraw),
        "text" => tool(board, Tool::Text),
        "image" => tool(board, Tool::Image),
        "frame" => tool(board, Tool::Frame),
        "eraser" => tool(board, Tool::Eraser),

        // --- the view ----------------------------------------------------------------------
        "zoom_in" => {
            board.viewport.zoom_by(crate::viewport::STEP, None);
            false
        }
        "zoom_out" => {
            board.viewport.zoom_by(1.0 / crate::viewport::STEP, None);
            false
        }
        "actual" => {
            board.viewport.actual();
            false
        }
        "fit" => {
            fit(board);
            false
        }
        "pan_left" => pan(board, Vec2::new(crate::viewport::NUDGE, 0.0)),
        "pan_right" => pan(board, Vec2::new(-crate::viewport::NUDGE, 0.0)),
        "pan_up" => pan(board, Vec2::new(0.0, crate::viewport::NUDGE)),
        "pan_down" => pan(board, Vec2::new(0.0, -crate::viewport::NUDGE)),

        // --- the selection -----------------------------------------------------------------
        "select_all" => {
            board.with_scene(|scene| {
                let all: Vec<Id> = scene
                    .alive()
                    .filter(|element| !element.locked)
                    .map(|element| element.id.clone())
                    .collect();
                scene.select(all);
            });
            false
        }
        "delete" => command(board, Command::Delete(selected())),
        "duplicate" => duplicate(board),
        "group" => command(board, Command::Group(selected())),
        "ungroup" => command(board, Command::Ungroup(selected())),
        "to_front" => reorder(board, Order::Front),
        "to_back" => reorder(board, Order::Back),
        "forward" => reorder(board, Order::Forward),
        "backward" => reorder(board, Order::Backward),
        "lock" => command(
            board,
            Command::Restyle {
                ids: selected(),
                change: Change::Locked(true),
            },
        ),
        "unlock" => command(
            board,
            Command::Restyle {
                ids: selected(),
                change: Change::Locked(false),
            },
        ),

        // --- moving what is selected -------------------------------------------------------
        "nudge_left" => nudge(board, Vec2::new(-1.0, 0.0)),
        "nudge_right" => nudge(board, Vec2::new(1.0, 0.0)),
        "nudge_up" => nudge(board, Vec2::new(0.0, -1.0)),
        "nudge_down" => nudge(board, Vec2::new(0.0, 1.0)),
        "shove_left" => nudge(board, Vec2::new(-SHOVE, 0.0)),
        "shove_right" => nudge(board, Vec2::new(SHOVE, 0.0)),
        "shove_up" => nudge(board, Vec2::new(0.0, -SHOVE)),
        "shove_down" => nudge(board, Vec2::new(0.0, SHOVE)),

        // --- the clipboard -----------------------------------------------------------------
        "copy" => {
            copy(board, false);
            false
        }
        "cut" => copy(board, true),
        "paste" => {
            // The desktop answers when it has looked, so the paste happens a moment later and this
            // cannot say whether it changed anything. The board's revision does.
            let board = *board;
            if let Some(clipboards) = zgui::prelude::try_use_clipboard() {
                clipboards.read_text(zgui::prelude::ClipboardKind::Standard, move |text| {
                    if let Some(text) = text {
                        // A library on the clipboard is stamped, not pasted: its items are
                        // groups to place, not elements to put back.
                        if crate::library::is_library(&text) {
                            crate::library::insert_first(&board, &text);
                        } else {
                            crate::clipboard::paste(&board, &text);
                        }
                    }
                });
            }
            false
        }

        // --- the panels --------------------------------------------------------------------
        "style" => {
            board.panel.update(|held| *held = !*held);
            false
        }
        "edit_text" => {
            crate::text::open_selected(board);
            false
        }
        "escape" => {
            escape(board);
            false
        }
        // Silently. The base map layers underneath the region, and an unbound key there falls
        // through to it.
        _ => false,
    }
}

/// Puts what is selected on the clipboard, and takes it away when `cut` asks.
fn copy(board: &Board, cut: bool) -> bool {
    let Some(text) = crate::clipboard::copied(board) else {
        return false;
    };
    if let Some(clipboards) = zgui::prelude::try_use_clipboard() {
        clipboards.set_text(zgui::prelude::ClipboardKind::Standard, text);
    }
    if !cut {
        return false;
    }
    let ids = board.read_untracked().selection().to_vec();
    command(board, Command::Delete(ids))
}

/// How far a shove moves the selection, against a nudge's one.
const SHOVE: f64 = 5.0;

/// Chooses a tool.
fn tool(board: &Board, tool: Tool) -> bool {
    board.tool.set(tool);
    false
}

/// Moves the view.
fn pan(board: &Board, by: Vec2) -> bool {
    board.viewport.pan_by(by);
    false
}

/// Moves what is selected.
fn nudge(board: &Board, by: Vec2) -> bool {
    let ids = board.read_untracked().selection().to_vec();
    command(board, Command::Translate { ids, by })
}

/// Does a command, unless there is nothing to do it to.
fn command(board: &Board, command: Command) -> bool {
    match &command {
        Command::Delete(ids)
        | Command::Group(ids)
        | Command::Ungroup(ids)
        | Command::Translate { ids, .. }
        | Command::Restyle { ids, .. }
            if ids.is_empty() =>
        {
            false
        }
        _ => board.apply(command),
    }
}

/// Moves what is selected through the order.
fn reorder(board: &Board, order: Order) -> bool {
    let ids = board.read_untracked().selection().to_vec();
    if ids.is_empty() {
        return false;
    }
    board.apply(Command::Reorder { ids, order })
}

/// Puts a copy of what is selected a little way off, and selects the copy.
fn duplicate(board: &Board) -> bool {
    let scene = board.read_untracked();
    if !scene.has_selection() {
        return false;
    }
    let chosen: Vec<Id> = scene.selection().to_vec();
    let mut copies = Vec::new();
    let mut names = Vec::new();
    for id in &chosen {
        let Some(at) = scene.drawing.find(id).map(|(at, _)| at) else {
            continue;
        };
        let Some(held) = scene
            .drawing
            .at(at)
            .and_then(|at| scene.drawing.store.element(at))
        else {
            continue;
        };
        copies.push(held.clone());
    }
    drop(scene);
    if copies.is_empty() {
        return false;
    }

    let mut made = Vec::with_capacity(copies.len());
    board.with_scene(|scene| {
        for mut copy in copies {
            let id = scene.fresh_id();
            let seed = scene.fresh_seed();
            let nonce = scene.fresh_nonce();
            copy.insert(
                "id".to_owned(),
                serde_json::Value::String(id.as_str().to_owned()),
            );
            copy.insert("seed".to_owned(), serde_json::Value::from(seed.0));
            copy.insert("versionNonce".to_owned(), serde_json::Value::from(nonce));
            // A copy on top of what it was copied from cannot be told apart, so it is put beside it.
            for key in ["x", "y"] {
                let held = copy
                    .get(key)
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(0.0);
                copy.insert(
                    key.to_owned(),
                    excalidraw::store::Number::json(held + OFFSET),
                );
            }
            // The copy takes a key of its own, put in when it is filed.
            copy.insert("index".to_owned(), serde_json::Value::Null);
            names.push(id);
            made.push(serde_json::Value::Object(copy));
        }
    });

    let moved = board.apply(Command::Insert(made));
    if moved {
        board.with_scene(|scene| scene.select(names));
    }
    moved
}

/// How far a copy is put from what it was copied from.
const OFFSET: f64 = 10.0;

/// Puts the drawing on screen.
pub fn fit(board: &Board) {
    let scene = board.read_untracked();
    let bounds = excalidraw::geom::of_many(scene.alive());
    drop(scene);
    board.viewport.fit(bounds);
}

/// Backs out of whatever is going on, one layer at a time.
fn escape(board: &Board) {
    if crate::pointer::finish_points(board) {
        return;
    }
    if board.live.get_untracked().is_some() {
        crate::pointer::cancel(board);
        return;
    }
    if board.editing.get_untracked().is_some() {
        board.editing.set(None);
        return;
    }
    if board.panel.get_untracked() {
        board.panel.set(false);
        return;
    }
    if board.tool.get_untracked() != Tool::Select {
        board.tool.set(Tool::Select);
        return;
    }
    board.with_scene(excalidraw::Scene::clear_selection);
}
