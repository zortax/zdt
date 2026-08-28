//! Writing the words in a drawing.
//!
//! A text element is edited where it is: a field is put over it at the size and place it is drawn,
//! and what is typed is written back when the field is left. Nothing else in the editor changes
//! while words are being typed, so the drawing under them stays exactly as it was.

mod composer;
mod measure;

use excalidraw::scene::build;
use excalidraw::{Command, Id, Kind};
use kurbo::Point;
use zgui::reactive::prelude::*;

use crate::state::{Board, Tool};

pub use self::composer::{Composer, ComposerProps};
pub use self::measure::{Measured, measure};

/// What a keystroke did to the words being typed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Typed {
    /// Nothing: no words are being typed, and the key is the drawing's.
    Idle,
    /// It was taken, and the words are still open.
    Taken,
    /// It finished them.
    Finished,
}

/// Gives one key to the words being typed.
///
/// The editor holds the keyboard, so every key comes through here first. A key that means nothing
/// to the words is [`Typed::Idle`] and belongs to the drawing.
pub fn key(board: &Board, key: &zgui::vocab::Key, shift: bool) -> Typed {
    use zgui::vocab::{Key, NamedKey};

    if !is_open(board) {
        return Typed::Idle;
    }
    match key {
        // A break, or the end of the words. Excalidraw finishes on escape and breaks on enter;
        // this does both, with shift for the break, so a stray enter does not swallow a line.
        Key::Named(NamedKey::Enter) if shift => {
            newline(board);
            Typed::Taken
        }
        Key::Named(NamedKey::Enter | NamedKey::Escape) => {
            finish(board);
            Typed::Finished
        }
        Key::Named(NamedKey::Backspace) => {
            backspace(board);
            Typed::Taken
        }
        Key::Named(NamedKey::Space) => {
            insert(board, " ");
            Typed::Taken
        }
        Key::Character(letter) => {
            insert(board, letter);
            Typed::Taken
        }
        // Everything else — the arrows, the function keys — is swallowed rather than let through:
        // a key that moved the selection while words were being typed would be a surprise.
        _ => Typed::Taken,
    }
}

/// Opens `id` for editing, with what it already says.
fn open(board: &Board, id: &Id) {
    let typed = board
        .read_untracked()
        .element(id)
        .and_then(|held| held.text().map(|words| words.original_text.clone()))
        .unwrap_or_default();
    board.typing.set(typed);
    board.editing.set(Some(id.clone()));
}

/// Whether words are being typed.
#[must_use]
pub fn is_open(board: &Board) -> bool {
    board.editing.get_untracked().is_some()
}

/// Puts `text` in at the end of what is being typed.
///
/// Answers whether it was taken. The editor has the keyboard, not the box the words are shown in,
/// so this is where a keystroke becomes a letter.
pub fn insert(board: &Board, text: &str) -> bool {
    if !is_open(board) || text.is_empty() {
        return false;
    }
    // A control character is a key, not a letter: the key handler has already dealt with it.
    if text.chars().any(|letter| letter.is_control()) {
        return false;
    }
    board.typing.update(|held| held.push_str(text));
    true
}

/// Takes the last letter back out.
pub fn backspace(board: &Board) -> bool {
    if !is_open(board) {
        return false;
    }
    board.typing.update(|held| {
        held.pop();
    });
    true
}

/// Puts a line break in.
pub fn newline(board: &Board) -> bool {
    if !is_open(board) {
        return false;
    }
    board.typing.update(|held| held.push('\n'));
    true
}

/// Finishes typing, keeping what was typed.
pub fn finish(board: &Board) -> bool {
    if !is_open(board) {
        return false;
    }
    let typed = board.typing.get_untracked();
    commit(board, &typed);
    // The text tool has done its one job, so the pointer goes back to choosing things.
    if board.tool.get_untracked() == Tool::Text {
        board.tool.set(Tool::Select);
    }
    true
}

/// Opens whatever is under `at`, in view pixels, for editing.
///
/// A press on a shape that can hold words makes a label inside it when it has none; a press on
/// nothing makes free words there.
pub fn open_at(board: &Board, at: Point) {
    let scene_at = board.viewport.scene_point(at);
    let tolerance = 10.0 / board.viewport.zoom_untracked().max(f64::EPSILON);
    let scene = board.read_untracked();
    let hit = scene.hit(scene_at, tolerance).cloned();
    drop(scene);

    match hit {
        Some(element) if element.kind == Kind::Text => board.editing.set(Some(element.id)),
        Some(element) if element.kind.is_text_container() => match element.bound_text().cloned() {
            Some(id) => board.editing.set(Some(id)),
            None => make(board, Some(&element.id), scene_at),
        },
        _ => make(board, None, scene_at),
    }
}

/// Opens the one selected text element, when exactly one is selected.
pub fn open_selected(board: &Board) {
    let scene = board.read_untracked();
    let chosen: Vec<Id> = scene.selection().to_vec();
    let held = (chosen.len() == 1)
        .then(|| chosen.first().and_then(|id| scene.element(id)))
        .flatten()
        .cloned();
    drop(scene);
    let Some(element) = held else {
        return;
    };
    match element.kind {
        Kind::Text => open(board, &element.id),
        kind if kind.is_text_container() => match element.bound_text().cloned() {
            Some(id) => open(board, &id),
            None => {
                let at = excalidraw::geom::bounds::center(&element);
                make(board, Some(&element.id), at);
            }
        },
        _ => {}
    }
}

/// Makes a text element at `at`, in the scene, and opens it.
fn make(board: &Board, container: Option<&Id>, at: Point) {
    let now = board.read_untracked().now();
    let style = board.style_for(Tool::Text);

    let mut id = None;
    let mut seed = None;
    let mut nonce = None;
    board.with_scene(|scene| {
        id = Some(scene.fresh_id());
        seed = Some(scene.fresh_seed());
        nonce = Some(scene.fresh_nonce());
    });
    let (Some(id), Some(seed), Some(nonce)) = (id, seed, nonce) else {
        return;
    };

    let height = style.font_size * style.font_family.line_height();
    let mut element = build::element(
        Kind::Text,
        build::Box_ {
            // The words start where the press was, not below and to the right of it.
            x: at.x,
            y: at.y - height / 2.0,
            width: 0.0,
            height,
        },
        &style,
        &id,
        seed,
        nonce,
        now,
    );
    if let (Some(container), Some(object)) = (container, element.as_object_mut()) {
        object.insert(
            "containerId".to_owned(),
            serde_json::Value::String(container.as_str().to_owned()),
        );
        // Words inside a shape wrap in it rather than growing past it.
        object.insert("autoResize".to_owned(), serde_json::Value::Bool(false));
    }

    if board.apply(Command::Insert(vec![element])) {
        if let Some(container) = container {
            bind_label(board, container, &id);
        }
        open(board, &id);
        if board.tool.get_untracked() == Tool::Text {
            board.tool.set(Tool::Select);
        }
    }
}

/// Notes on the shape that the words are written inside it.
fn bind_label(board: &Board, container: &Id, label: &Id) {
    let scene = board.read_untracked();
    let Some(at) = scene
        .drawing
        .find(container)
        .and_then(|(at, _)| scene.drawing.at(at))
    else {
        return;
    };
    let mut bound: Vec<serde_json::Value> = scene
        .drawing
        .store
        .element(at)
        .and_then(|held| held.get("boundElements"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    drop(scene);

    let mut entry = serde_json::Map::new();
    entry.insert(
        "id".to_owned(),
        serde_json::Value::String(label.as_str().to_owned()),
    );
    entry.insert(
        "type".to_owned(),
        serde_json::Value::String("text".to_owned()),
    );
    bound.push(serde_json::Value::Object(entry));
    board.with_scene(|scene| {
        scene
            .drawing
            .store
            .patch(at, "boundElements", serde_json::Value::Array(bound));
        scene.drawing.reread();
    });
}

/// Writes what has been typed and closes the editor.
///
/// Words that were left empty are taken away again, so a press that opened one by accident leaves
/// nothing behind.
pub fn commit(board: &Board, typed: &str) {
    let Some(id) = board.editing.get_untracked() else {
        return;
    };
    board.editing.set(None);
    board.typing.set(String::new());

    let scene = board.read_untracked();
    let Some(element) = scene.element(&id).cloned() else {
        return;
    };
    let container = element
        .text()
        .and_then(|words| words.container_id.clone())
        .and_then(|id| scene.element(&id).cloned());
    drop(scene);

    if typed.trim().is_empty() {
        board.apply(Command::Delete(vec![id]));
        return;
    }

    let Some(measured) = measure(&element, container.as_ref(), typed) else {
        return;
    };
    board.apply(Command::SetText {
        id: id.clone(),
        text: measured.wrapped,
        original_text: typed.to_owned(),
        width: measured.width,
        height: measured.height,
    });

    // A shape that is now too small for its label grows to hold it.
    if let Some(container) = container {
        grow(board, &container, measured.width, measured.height);
    }
}

/// Makes `container` large enough to hold words that size.
fn grow(board: &Board, container: &excalidraw::Element, width: f64, height: f64) {
    let needed_width = excalidraw::text::container_size_for(width, container.kind);
    let needed_height = excalidraw::text::container_size_for(height, container.kind);
    if container.width >= needed_width && container.height >= needed_height {
        return;
    }
    let to = kurbo::Rect::new(
        container.x,
        container.y,
        container.x + container.width.max(needed_width),
        container.y + container.height.max(needed_height),
    );
    let from = kurbo::Rect::new(
        container.x,
        container.y,
        container.x + container.width,
        container.y + container.height,
    );
    board.apply(Command::Resize {
        ids: vec![container.id.clone()],
        from,
        to,
    });
}
