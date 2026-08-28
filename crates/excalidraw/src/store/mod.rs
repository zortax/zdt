//! The JSON a drawing was read from, kept so that writing it back changes as little as possible.
//!
//! An element is read into a view the rest of this crate works in, but the view is not what is
//! written: a command patches the keys it touched in the object the element was read from, and
//! everything else — the key order, the way a number was written, keys this crate has never heard
//! of — survives untouched. That is what keeps a saved drawing a small diff rather than a rewritten
//! file.

mod write;

use serde_json::{Map, Value};

pub use self::write::{Number, to_string_pretty};

/// The objects a drawing's elements were read from, in the order they are painted.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Store {
    /// The whole file, with `elements` still inside it.
    document: Value,
}

/// Where one element sits in the store.
pub type At = usize;

impl Store {
    /// The store `document` is.
    ///
    /// The document is taken as it is. A document that is not an object, or that holds no
    /// `elements` array, still stores: it simply holds no elements.
    #[must_use]
    pub const fn new(document: Value) -> Self {
        Self { document }
    }

    /// An empty drawing.
    #[must_use]
    pub fn empty() -> Self {
        Self::new(Value::Object(Map::new()))
    }

    /// The whole document.
    #[must_use]
    pub const fn document(&self) -> &Value {
        &self.document
    }

    /// The whole document, to be changed.
    pub const fn document_mut(&mut self) -> &mut Value {
        &mut self.document
    }

    /// The elements, as the objects they were read from.
    #[must_use]
    pub fn elements(&self) -> &[Value] {
        self.document
            .get("elements")
            .and_then(Value::as_array)
            .map_or(&[], Vec::as_slice)
    }

    /// How many there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.elements().len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The object element `at` was read from.
    #[must_use]
    pub fn element(&self, at: At) -> Option<&Map<String, Value>> {
        self.elements().get(at).and_then(Value::as_object)
    }

    /// The same, to be changed.
    pub fn element_mut(&mut self, at: At) -> Option<&mut Map<String, Value>> {
        self.array_mut().get_mut(at).and_then(Value::as_object_mut)
    }

    /// Writes `value` at `key` on element `at`, and answers whether anything moved.
    ///
    /// A key that is already this value is left alone, so a command that changes nothing writes
    /// nothing and the file's bytes do not move.
    pub fn patch(&mut self, at: At, key: &str, value: Value) -> bool {
        let Some(object) = self.element_mut(at) else {
            return false;
        };
        if object.get(key) == Some(&value) {
            return false;
        }
        object.insert(key.to_owned(), value);
        true
    }

    /// Writes several keys at once, and answers whether any moved.
    pub fn patch_all(&mut self, at: At, values: impl IntoIterator<Item = (String, Value)>) -> bool {
        let mut moved = false;
        for (key, value) in values {
            moved |= self.patch(at, &key, value);
        }
        moved
    }

    /// Puts `element` at the end of the order.
    pub fn push(&mut self, element: Value) -> At {
        let array = self.array_mut();
        array.push(element);
        array.len() - 1
    }

    /// Puts it at `at`, moving everything from there along.
    pub fn insert(&mut self, at: At, element: Value) {
        let array = self.array_mut();
        let at = at.min(array.len());
        array.insert(at, element);
    }

    /// Takes the element at `at` out of the order.
    pub fn remove(&mut self, at: At) -> Option<Value> {
        let array = self.array_mut();
        (at < array.len()).then(|| array.remove(at))
    }

    /// Moves the element at `from` to `to`, carrying everything between it along.
    pub fn shift(&mut self, from: At, to: At) {
        let array = self.array_mut();
        if from >= array.len() || to >= array.len() || from == to {
            return;
        }
        let held = array.remove(from);
        array.insert(to, held);
    }

    /// Puts the elements in `order`, which names each one's place by where it is now.
    pub fn reorder(&mut self, order: &[At]) {
        let array = self.array_mut();
        if order.len() != array.len() {
            return;
        }
        let mut taken: Vec<Option<Value>> = std::mem::take(array).into_iter().map(Some).collect();
        for at in order {
            if let Some(held) = taken.get_mut(*at).and_then(Option::take) {
                array.push(held);
            }
        }
    }

    /// The elements array, made if the document has none.
    fn array_mut(&mut self) -> &mut Vec<Value> {
        if !self.document.is_object() {
            self.document = Value::Object(Map::new());
        }
        let object = self
            .document
            .as_object_mut()
            .expect("the document was just made an object");
        object
            .entry("elements")
            .or_insert_with(|| Value::Array(Vec::new()));
        let held = object
            .get_mut("elements")
            .expect("the entry was just inserted");
        if !held.is_array() {
            *held = Value::Array(Vec::new());
        }
        held.as_array_mut().expect("just made an array")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        Store::new(serde_json::json!({
            "type": "excalidraw",
            "elements": [
                { "type": "rectangle", "id": "a", "x": 0, "y": 0 },
                { "type": "ellipse", "id": "b", "x": 10, "y": 10 },
            ],
        }))
    }

    #[test]
    fn a_patch_writes_one_key_and_leaves_the_rest() {
        let mut store = store();
        assert!(store.patch(0, "x", serde_json::json!(42)));
        let held = store.element(0).expect("the first element");
        assert_eq!(held.get("x"), Some(&serde_json::json!(42)));
        assert_eq!(held.get("id"), Some(&serde_json::json!("a")));
        assert_eq!(held.len(), 4, "no key was added or dropped");
    }

    #[test]
    fn writing_the_value_that_is_already_there_moves_nothing() {
        let mut store = store();
        assert!(!store.patch(0, "x", serde_json::json!(0)));
    }

    #[test]
    fn a_new_key_keeps_its_place_at_the_end() {
        let mut store = store();
        store.patch(0, "locked", serde_json::json!(true));
        let keys: Vec<&str> = store
            .element(0)
            .expect("the first element")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["type", "id", "x", "y", "locked"]);
    }

    #[test]
    fn shifting_moves_one_element_and_carries_the_rest() {
        let mut store = store();
        store.shift(0, 1);
        assert_eq!(
            store.element(0).expect("the first").get("id"),
            Some(&serde_json::json!("b"))
        );
    }

    #[test]
    fn a_document_with_no_elements_still_takes_one() {
        let mut store = Store::new(serde_json::json!({ "type": "excalidraw" }));
        assert!(store.is_empty());
        store.push(serde_json::json!({ "type": "rectangle" }));
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn reordering_puts_every_element_where_it_was_asked_for() {
        let mut store = store();
        store.reorder(&[1, 0]);
        assert_eq!(
            store.element(0).expect("the first").get("id"),
            Some(&serde_json::json!("b"))
        );
        assert_eq!(
            store.element(1).expect("the second").get("id"),
            Some(&serde_json::json!("a"))
        );
    }
}
