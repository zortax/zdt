//! What has already been drawn.
//!
//! Turning an element into shapes costs a walk of the drawing library, and a drawing redraws every
//! frame something moves. An element is unchanged as long as its id and its version are, so that
//! pair is the key, and the shapes behind it are shared rather than rebuilt.

use std::rc::Rc;

use rustc_hash::FxHashMap;

use crate::element::{Element, Id};

use super::Piece;

/// The shapes each element was last drawn as.
#[derive(Debug, Default)]
pub struct Cache {
    held: FxHashMap<Id, (u64, Rc<Vec<Piece>>)>,
}

impl Cache {
    /// Nothing drawn yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The shapes `element` is, drawing it only if it has changed.
    pub fn pieces(&mut self, element: &Element) -> Rc<Vec<Piece>> {
        if let Some((version, held)) = self.held.get(&element.id)
            && *version == element.version
        {
            return Rc::clone(held);
        }
        let drawn = Rc::new(super::pieces(element));
        self.held
            .insert(element.id.clone(), (element.version, Rc::clone(&drawn)));
        drawn
    }

    /// Forgets everything but the elements still in `elements`.
    ///
    /// Called after a change that removed something, so a drawing edited for an hour does not keep
    /// every shape it ever held.
    pub fn retain<'a>(&mut self, elements: impl IntoIterator<Item = &'a Element>) {
        let alive: rustc_hash::FxHashSet<&Id> =
            elements.into_iter().map(|element| &element.id).collect();
        self.held.retain(|id, _| alive.contains(id));
    }

    /// How many elements are remembered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.held.len()
    }

    /// Whether none are.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }

    /// Forgets everything.
    pub fn clear(&mut self) {
        self.held.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(json: &str) -> Element {
        let value: serde_json::Value = serde_json::from_str(json).expect("valid JSON");
        crate::element::read(value.as_object().expect("an object")).expect("an element")
    }

    #[test]
    fn an_unchanged_element_is_not_drawn_again() {
        let mut cache = Cache::new();
        let held = read(r#"{"type":"rectangle","id":"a","width":100,"height":50,"seed":1}"#);
        let first = cache.pieces(&held);
        let second = cache.pieces(&held);
        assert!(Rc::ptr_eq(&first, &second));
    }

    #[test]
    fn a_changed_element_is() {
        let mut cache = Cache::new();
        let before = read(r#"{"type":"rectangle","id":"a","width":100,"height":50,"seed":1}"#);
        let after =
            read(r#"{"type":"rectangle","id":"a","width":200,"height":50,"seed":1,"version":2}"#);
        let first = cache.pieces(&before);
        let second = cache.pieces(&after);
        assert!(!Rc::ptr_eq(&first, &second));
        assert_eq!(cache.len(), 1, "the old drawing was replaced, not kept");
    }

    #[test]
    fn what_is_gone_is_forgotten() {
        let mut cache = Cache::new();
        let one = read(r#"{"type":"rectangle","id":"a","width":10,"height":10,"seed":1}"#);
        let two = read(r#"{"type":"rectangle","id":"b","width":10,"height":10,"seed":1}"#);
        cache.pieces(&one);
        cache.pieces(&two);
        assert_eq!(cache.len(), 2);
        cache.retain([&one]);
        assert_eq!(cache.len(), 1);
        cache.clear();
        assert!(cache.is_empty());
    }
}
