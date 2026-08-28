//! Stamping something out of a library into a drawing.
//!
//! A `.excalidrawlib` file is a set of small groups of elements. Inserting one puts a fresh copy
//! into the drawing, named anew and grouped together, so it can be moved as one thing and stamped
//! again without the two copies sharing anything.

use excalidraw::file::library::{Item, Library};
use excalidraw::{Command, Id};
use kurbo::Point;
use serde_json::Value;

use crate::state::Board;

/// Why a library could not be read.
pub use excalidraw::file::Error;

/// The library `text` holds.
///
/// # Errors
///
/// If the text is not JSON, or is JSON that does not call itself a library.
pub fn parse(text: &str) -> Result<Library, Error> {
    excalidraw::file::library::parse(text)
}

/// Whether `text` is a library at all.
#[must_use]
pub fn is_library(text: &str) -> bool {
    parse(text).is_ok()
}

/// Puts `item` into the drawing, its middle at `at` in the scene.
///
/// Answers whether anything was added. The copy is selected, so it can be moved straight away.
pub fn insert(board: &Board, item: &Item, at: Point) -> bool {
    if item.parsed.is_empty() {
        return false;
    }
    // Where the item sits now, so it can be moved to where it was asked for.
    let Some(bounds) = excalidraw::geom::of_many(item.parsed.iter()) else {
        return false;
    };
    let by = at - bounds.center();

    // Names first, so a binding inside the item points at the copy rather than at the original.
    let mut renamed: rustc_hash::FxHashMap<String, Id> = rustc_hash::FxHashMap::default();
    let mut group = None;
    board.with_scene(|scene| {
        for element in &item.elements {
            if let Some(id) = element.get("id").and_then(Value::as_str) {
                renamed.insert(id.to_owned(), scene.fresh_id());
            }
        }
        group = Some(scene.fresh_id());
    });
    let Some(group) = group else {
        return false;
    };

    let mut made = Vec::with_capacity(item.elements.len());
    let mut names = Vec::with_capacity(item.elements.len());
    board.with_scene(|scene| {
        for element in &item.elements {
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
            copy.insert("isDeleted".to_owned(), Value::Bool(false));
            // Everything stamped together is one thing, so a click takes hold of all of it.
            copy.insert(
                "groupIds".to_owned(),
                Value::Array(vec![Value::String(group.as_str().to_owned())]),
            );
            rename_bindings(&mut copy, &renamed);
            for (key, delta) in [("x", by.x), ("y", by.y)] {
                let held = copy.get(key).and_then(Value::as_f64).unwrap_or(0.0);
                copy.insert(
                    key.to_owned(),
                    excalidraw::store::Number::json(held + delta),
                );
            }
            names.push(id);
            made.push(Value::Object(copy));
        }
    });
    if made.is_empty() {
        return false;
    }

    let moved = board.apply(Command::Insert(made));
    if moved {
        board.with_scene(|scene| scene.select(names));
    }
    moved
}

/// Points every name inside the item at the copy.
fn rename_bindings(
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
}

/// Puts the first item of the library `text` holds into the middle of the view.
///
/// Answers whether anything was added. This is what a paste of a library file does.
pub fn insert_first(board: &Board, text: &str) -> bool {
    let Ok(library) = parse(text) else {
        return false;
    };
    let Some(item) = library.items.first() else {
        return false;
    };
    let (width, height) = board.viewport.size();
    let middle = board
        .viewport
        .scene_point(Point::new(width / 2.0, height / 2.0));
    insert(board, item, middle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> Board {
        let drawing =
            excalidraw::file::parse(r#"{"type":"excalidraw","elements":[]}"#).expect("a drawing");
        let board = Board::new(excalidraw::Scene::new(drawing, 1, 1));
        board.viewport.set_size(800.0, 600.0);
        board
    }

    const LIBRARY: &str = r#"{"type":"excalidrawlib","version":2,"libraryItems":[
        {"id":"one","name":"a box and a label","elements":[
            {"type":"rectangle","id":"r","x":0,"y":0,"width":100,"height":50,
             "boundElements":[{"id":"t","type":"text"}]},
            {"type":"text","id":"t","x":10,"y":10,"width":80,"height":25,"text":"hi",
             "containerId":"r"}]}]}"#;

    #[test]
    fn a_library_is_recognised_and_a_drawing_is_not() {
        assert!(is_library(LIBRARY));
        assert!(!is_library(r#"{"type":"excalidraw","elements":[]}"#));
        assert!(!is_library("hello"));
    }

    #[test]
    fn stamping_an_item_puts_a_fresh_copy_where_it_was_asked_for() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let board = board();
            let library = parse(LIBRARY).expect("a library");
            assert!(insert(&board, &library.items[0], Point::new(500.0, 400.0)));

            let scene = board.read_untracked();
            assert_eq!(scene.elements().len(), 2);
            // Nothing kept the name it was written with.
            assert!(scene.elements().iter().all(|held| held.id.as_str() != "r"));
            // And the copy is where it was asked for.
            let bounds = excalidraw::geom::of_many(scene.alive()).expect("a box");
            assert!((bounds.center() - Point::new(500.0, 400.0)).hypot() < 1.0);
        });
    }

    #[test]
    fn everything_stamped_together_is_one_group() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let board = board();
            let library = parse(LIBRARY).expect("a library");
            insert(&board, &library.items[0], Point::ZERO);

            let scene = board.read_untracked();
            let groups: Vec<&str> = scene
                .elements()
                .iter()
                .filter_map(excalidraw::Element::outermost_group)
                .collect();
            assert_eq!(groups.len(), 2);
            assert_eq!(groups[0], groups[1], "one group, not two");
        });
    }

    #[test]
    fn a_label_inside_the_item_still_names_its_container() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let board = board();
            let library = parse(LIBRARY).expect("a library");
            insert(&board, &library.items[0], Point::ZERO);

            let scene = board.read_untracked();
            let shape = scene
                .elements()
                .iter()
                .find(|held| held.kind == excalidraw::Kind::Rectangle)
                .expect("the shape");
            let label = scene
                .elements()
                .iter()
                .find(|held| held.kind == excalidraw::Kind::Text)
                .expect("the label");
            assert_eq!(
                label.text().expect("words").container_id.as_ref(),
                Some(&shape.id),
                "the copy's label names the copy's shape"
            );
            assert_eq!(shape.bound_text(), Some(&label.id));
        });
    }

    #[test]
    fn stamping_the_first_item_puts_it_in_the_middle_of_the_view() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let board = board();
            assert!(insert_first(&board, LIBRARY));
            let scene = board.read_untracked();
            let bounds = excalidraw::geom::of_many(scene.alive()).expect("a box");
            assert!((bounds.center() - Point::new(400.0, 300.0)).hypot() < 1.0);
        });
    }

    #[test]
    fn anything_that_is_not_a_library_stamps_nothing() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let board = board();
            assert!(!insert_first(&board, "hello"));
            assert!(!insert_first(
                &board,
                r#"{"type":"excalidrawlib","version":2,"libraryItems":[]}"#
            ));
        });
    }
}
