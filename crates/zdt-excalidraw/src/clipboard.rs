//! Copying and pasting.
//!
//! What goes on the clipboard is the JSON the Excalidraw web app puts there, so a copy crosses
//! between them: what is copied here pastes there, and what is copied there pastes here.

use excalidraw::clipboard::Payload;
use excalidraw::{Command, Id};
use kurbo::Vec2;
use serde_json::Value;

use crate::state::Board;

/// How far a paste is put from where the thing it copied sits, when it lands on top of it.
const OFFSET: f64 = 10.0;

/// The text a copy of what is selected puts on the clipboard.
///
/// Answers nothing when nothing is selected.
#[must_use]
pub fn copied(board: &Board) -> Option<String> {
    let scene = board.read_untracked();
    let chosen: Vec<Id> = scene.selection().to_vec();
    if chosen.is_empty() {
        return None;
    }
    let mut elements = Vec::with_capacity(chosen.len());
    let mut files = excalidraw::file::Files::default();
    for id in &chosen {
        let Some(at) = scene
            .drawing
            .find(id)
            .map(|(at, _)| at)
            .and_then(|at| scene.drawing.at(at))
        else {
            continue;
        };
        let Some(held) = scene.drawing.store.element(at) else {
            continue;
        };
        // A picture's bytes travel with it, or a paste elsewhere would be an empty box.
        if let Some(file) = held
            .get("fileId")
            .and_then(Value::as_str)
            .and_then(|id| scene.drawing.files.get(id).map(|file| (id, file)))
        {
            files.insert(file.0.to_owned(), file.1.clone());
        }
        elements.push(Value::Object(held.clone()));
    }
    if elements.is_empty() {
        return None;
    }
    Payload { elements, files }.to_string().ok()
}

/// Pastes what `text` carries, and answers whether anything was added.
///
/// Every element is given a fresh name, so a paste into the drawing it was copied from is two
/// things rather than one thing twice.
pub fn paste(board: &Board, text: &str) -> bool {
    let Ok(payload) = excalidraw::clipboard::parse(text) else {
        return false;
    };
    if payload.is_empty() {
        return false;
    }

    // Names are made before anything is written, so an arrow bound inside the paste is rebound to
    // the copy of the shape rather than to the shape it was copied from.
    let mut renamed: rustc_hash::FxHashMap<String, Id> = rustc_hash::FxHashMap::default();
    board.with_scene(|scene| {
        for element in &payload.elements {
            if let Some(id) = element.get("id").and_then(Value::as_str) {
                renamed.insert(id.to_owned(), scene.fresh_id());
            }
        }
    });

    let mut made = Vec::with_capacity(payload.elements.len());
    let mut names = Vec::with_capacity(payload.elements.len());
    board.with_scene(|scene| {
        for element in &payload.elements {
            let Some(object) = element.as_object() else {
                continue;
            };
            let mut copy = object.clone();
            let Some(id) = copy
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| renamed.get(id))
                .cloned()
            else {
                continue;
            };
            copy.insert("id".to_owned(), Value::String(id.as_str().to_owned()));
            copy.insert("seed".to_owned(), Value::from(scene.fresh_seed().0));
            copy.insert("versionNonce".to_owned(), Value::from(scene.fresh_nonce()));
            copy.insert("version".to_owned(), Value::from(1));
            copy.insert("index".to_owned(), Value::Null);
            copy.insert("updated".to_owned(), Value::from(scene.now()));
            rename_references(&mut copy, &renamed);
            for key in ["x", "y"] {
                let held = copy.get(key).and_then(Value::as_f64).unwrap_or(0.0);
                copy.insert(
                    key.to_owned(),
                    excalidraw::store::Number::json(held + OFFSET),
                );
            }
            names.push(id);
            made.push(Value::Object(copy));
        }
    });
    if made.is_empty() {
        return false;
    }

    // The pictures come first, so an image element that lands has bytes to draw.
    if !payload.files.is_empty() {
        board.with_scene(|scene| {
            let files = scene
                .drawing
                .store
                .document_mut()
                .as_object_mut()
                .and_then(|held| {
                    held.entry("files")
                        .or_insert_with(|| Value::Object(serde_json::Map::new()))
                        .as_object_mut()
                });
            if let Some(files) = files {
                for id in payload.files.ids() {
                    if let Some(file) = payload.files.get(id) {
                        files.insert(id.to_owned(), excalidraw::file::Files::entry_json(id, file));
                    }
                }
            }
            scene.drawing.reread();
        });
    }

    let moved = board.apply(Command::Insert(made));
    if moved {
        board.with_scene(|scene| scene.select(names));
    }
    moved
}

/// Points every name inside the paste at the copy rather than at the original.
fn rename_references(
    element: &mut serde_json::Map<String, Value>,
    renamed: &rustc_hash::FxHashMap<String, Id>,
) {
    let rename = |value: &Value| -> Option<Value> {
        let id = value.as_str()?;
        renamed
            .get(id)
            .map(|held| Value::String(held.as_str().to_owned()))
    };

    for key in ["containerId", "frameId"] {
        if let Some(held) = element.get(key).and_then(rename) {
            element.insert(key.to_owned(), held);
        }
    }
    for key in ["startBinding", "endBinding"] {
        let held = element.get(key).and_then(Value::as_object).cloned();
        if let Some(mut binding) = held {
            match binding.get("elementId").and_then(rename) {
                Some(id) => {
                    binding.insert("elementId".to_owned(), id);
                    element.insert(key.to_owned(), Value::Object(binding));
                }
                // What it was bound to was not copied, so it comes across unbound.
                None => {
                    element.insert(key.to_owned(), Value::Null);
                }
            }
        }
    }
    if let Some(bound) = element
        .get("boundElements")
        .and_then(Value::as_array)
        .cloned()
    {
        let kept: Vec<Value> = bound
            .into_iter()
            .filter_map(|held| {
                let mut entry = held.as_object()?.clone();
                let id = entry.get("id").and_then(rename)?;
                entry.insert("id".to_owned(), id);
                Some(Value::Object(entry))
            })
            .collect();
        element.insert("boundElements".to_owned(), Value::Array(kept));
    }
    // A group that was only partly copied would tie the copy to elements that did not come, so
    // every group name in a paste is a new one.
    if let Some(groups) = element.get("groupIds").and_then(Value::as_array).cloned() {
        let kept: Vec<Value> = groups
            .into_iter()
            .filter_map(|held| {
                let id = held.as_str()?;
                Some(Value::String(format!("{id}-pasted")))
            })
            .collect();
        element.insert("groupIds".to_owned(), Value::Array(kept));
    }
}

/// Moves what is selected by `by`, which a paste under the pointer would do.
pub fn nudge_pasted(board: &Board, by: Vec2) -> bool {
    let ids: Vec<Id> = board.read_untracked().selection().to_vec();
    if ids.is_empty() {
        return false;
    }
    board.apply(Command::Translate { ids, by })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Board;

    fn board(elements: &str) -> Board {
        let text = format!(r#"{{"type":"excalidraw","version":2,"elements":{elements}}}"#);
        let drawing = excalidraw::file::parse(&text).expect("a drawing");
        Board::new(excalidraw::Scene::new(drawing, 1, 1))
    }

    #[test]
    fn nothing_selected_copies_nothing() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let board = board(r#"[{"type":"rectangle","id":"a"}]"#);
            assert!(copied(&board).is_none());
        });
    }

    #[test]
    fn what_is_copied_pastes_back_as_something_new() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let board =
                board(r#"[{"type":"rectangle","id":"a","x":0,"y":0,"width":10,"height":10}]"#);
            board.with_scene(|scene| scene.select([Id::new("a")]));
            let text = copied(&board).expect("a copy");
            assert!(paste(&board, &text));

            let scene = board.read_untracked();
            assert_eq!(scene.elements().len(), 2);
            assert_ne!(scene.elements()[1].id.as_str(), "a");
            // It lands beside what it was copied from, not on top of it.
            assert!((scene.elements()[1].x - OFFSET).abs() < 1e-9);
            assert_eq!(scene.selection().len(), 1);
        });
    }

    #[test]
    fn an_arrow_bound_inside_the_paste_follows_the_copy() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let board = board(
                r#"[{"type":"rectangle","id":"box","width":50,"height":50},
                    {"type":"arrow","id":"arr","points":[[0,0],[10,0]],
                     "startBinding":{"elementId":"box","fixedPoint":[1,0.5001],"mode":"orbit"}}]"#,
            );
            board.with_scene(|scene| scene.select([Id::new("box"), Id::new("arr")]));
            let text = copied(&board).expect("a copy");
            assert!(paste(&board, &text));

            let scene = board.read_untracked();
            let arrow = scene
                .elements()
                .iter()
                .rev()
                .find(|held| held.kind == excalidraw::Kind::Arrow)
                .expect("the copy");
            let binding = arrow
                .linear()
                .expect("an arrow")
                .start_binding
                .as_ref()
                .expect("still bound");
            assert_ne!(binding.element.as_str(), "box", "it follows the copy");
        });
    }

    #[test]
    fn an_arrow_bound_to_something_left_behind_comes_across_unbound() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let board = board(
                r#"[{"type":"rectangle","id":"box","width":50,"height":50},
                    {"type":"arrow","id":"arr","points":[[0,0],[10,0]],
                     "startBinding":{"elementId":"box","fixedPoint":[1,0.5001],"mode":"orbit"}}]"#,
            );
            board.with_scene(|scene| scene.select([Id::new("arr")]));
            let text = copied(&board).expect("a copy");
            assert!(paste(&board, &text));

            let scene = board.read_untracked();
            let arrow = scene.elements().last().expect("the copy");
            assert!(arrow.linear().expect("an arrow").start_binding.is_none());
        });
    }

    #[test]
    fn a_picture_travels_with_the_element_that_draws_it() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let text = r#"{"type":"excalidraw","version":2,
                "elements":[{"type":"image","id":"i","width":10,"height":10,"fileId":"abc"}],
                "files":{"abc":{"mimeType":"image/png","dataURL":"data:image/png;base64,AAA=",
                                "created":1}}}"#;
            let drawing = excalidraw::file::parse(text).expect("a drawing");
            let held = Board::new(excalidraw::Scene::new(drawing, 1, 1));
            held.with_scene(|scene| scene.select([Id::new("i")]));
            let copy = copied(&held).expect("a copy");
            assert!(copy.contains("dataURL"));

            let empty = board(r#"[]"#);
            assert!(paste(&empty, &copy));
            assert_eq!(empty.read_untracked().drawing.files.len(), 1);
        });
    }

    #[test]
    fn anything_that_is_not_a_paste_pastes_nothing() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let board = board("[]");
            assert!(!paste(&board, "hello"));
            assert!(!paste(
                &board,
                r#"{"type":"excalidraw/clipboard","elements":[]}"#
            ));
        });
    }
}
