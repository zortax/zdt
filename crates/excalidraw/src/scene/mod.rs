//! A drawing, and what a session does to it.
//!
//! A [`Scene`] is the drawing plus what is selected in it. Everything that changes it goes through
//! [`Command`], and every command writes into the store — so a caller that applies a command and
//! then writes the file gets exactly the change and nothing else.
//!
//! A command is one undo step. A drag holds its own state and produces one command when the pointer
//! comes up, rather than a command per movement.

pub mod build;
mod command;
mod order;
mod style;

use kurbo::Point;
use rustc_hash::FxHashSet;

use crate::element::{Element, Id};
use crate::file::Drawing;
use crate::geom;

pub use self::build::{Box_, Style};
pub use self::command::Command;
pub use self::order::Order;
pub use self::style::Change;

/// A drawing, and what is selected in it.
#[derive(Clone, Debug)]
pub struct Scene {
    /// The drawing.
    pub drawing: Drawing,
    /// What is selected, by id.
    selection: Vec<Id>,
    /// What fresh ids, seeds and nonces come from.
    random: excalidraw_rough::Random,
    /// What a new element is given.
    pub style: Style,
    /// The clock a change is stamped with, as milliseconds since the epoch.
    now: u64,
}

impl Scene {
    /// The scene `drawing` is, with nothing selected.
    ///
    /// `seed` is what fresh ids and seeds come from, and `now` is the clock a change is stamped
    /// with — both the caller's, so a test can hold them still.
    #[must_use]
    pub fn new(drawing: Drawing, seed: u32, now: u64) -> Self {
        Self {
            drawing,
            selection: Vec::new(),
            random: excalidraw_rough::Random::new(seed),
            style: Style::default(),
            now,
        }
    }

    /// An empty drawing.
    #[must_use]
    pub fn empty(seed: u32, now: u64) -> Self {
        Self::new(Drawing::new(), seed, now)
    }

    /// The elements, in painting order.
    #[must_use]
    pub fn elements(&self) -> &[Element] {
        &self.drawing.elements
    }

    /// The ones that are not deleted, in painting order.
    pub fn alive(&self) -> impl Iterator<Item = &Element> {
        self.drawing
            .elements
            .iter()
            .filter(|element| !element.is_deleted)
    }

    /// The element `id` names.
    #[must_use]
    pub fn element(&self, id: &Id) -> Option<&Element> {
        self.drawing.find(id).map(|(_, held)| held)
    }

    /// What is selected, by id.
    #[must_use]
    pub fn selection(&self) -> &[Id] {
        &self.selection
    }

    /// The selected elements, in painting order.
    pub fn selected(&self) -> impl Iterator<Item = &Element> {
        let chosen: FxHashSet<&Id> = self.selection.iter().collect();
        self.drawing
            .elements
            .iter()
            .filter(move |element| chosen.contains(&element.id))
    }

    /// Whether `id` is selected.
    #[must_use]
    pub fn is_selected(&self, id: &Id) -> bool {
        self.selection.iter().any(|held| held == id)
    }

    /// Selects exactly these, and everything grouped with them.
    pub fn select(&mut self, ids: impl IntoIterator<Item = Id>) {
        self.selection = self.with_groups(ids);
    }

    /// Adds these to what is selected.
    pub fn add_to_selection(&mut self, ids: impl IntoIterator<Item = Id>) {
        for id in self.with_groups(ids) {
            if !self.is_selected(&id) {
                self.selection.push(id);
            }
        }
    }

    /// Selects nothing.
    pub fn clear_selection(&mut self) {
        self.selection.clear();
    }

    /// Whether anything is selected.
    #[must_use]
    pub fn has_selection(&self) -> bool {
        !self.selection.is_empty()
    }

    /// The rectangle the selection takes, when anything is selected.
    #[must_use]
    pub fn selection_bounds(&self) -> Option<geom::Bounds> {
        geom::of_many(self.selected())
    }

    /// These ids, and every element grouped with one of them.
    ///
    /// Taking hold of one member of a group takes hold of the group, which is what makes a group a
    /// group.
    fn with_groups(&self, ids: impl IntoIterator<Item = Id>) -> Vec<Id> {
        let asked: Vec<Id> = ids.into_iter().collect();
        let groups: FxHashSet<&str> = asked
            .iter()
            .filter_map(|id| self.element(id))
            .filter_map(Element::outermost_group)
            .collect();
        if groups.is_empty() {
            return asked;
        }
        let mut out = Vec::new();
        for element in self.alive() {
            let in_group = element
                .outermost_group()
                .is_some_and(|group| groups.contains(group));
            if in_group || asked.contains(&element.id) {
                out.push(element.id.clone());
            }
        }
        out
    }

    /// The top-most element under `at`, in the scene's coordinates.
    #[must_use]
    pub fn hit(&self, at: Point, tolerance: f64) -> Option<&Element> {
        crate::hit::top_most(&self.drawing.elements, at, tolerance)
            .map(|at| &self.drawing.elements[at])
    }

    /// The clock a change is stamped with.
    #[must_use]
    pub const fn now(&self) -> u64 {
        self.now
    }

    /// Moves the clock on, which a host does as time passes.
    pub const fn set_now(&mut self, now: u64) {
        self.now = now;
    }

    /// A fresh id.
    pub fn fresh_id(&mut self) -> Id {
        Id::fresh(&mut self.random)
    }

    /// A fresh seed.
    pub fn fresh_seed(&mut self) -> crate::element::Seed {
        crate::element::Seed::fresh(&mut self.random)
    }

    /// A fresh nonce, for the tie between two changes made at once.
    pub fn fresh_nonce(&mut self) -> u64 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let held = (self.random.next() * 2_147_483_648.0) as u64;
        held
    }

    /// Does `command`, and answers whether anything changed.
    ///
    /// Nothing changing is not a failure: a drag that ended where it began writes nothing, which is
    /// what keeps it out of the undo history and out of the file.
    pub fn apply(&mut self, command: Command) -> bool {
        let moved = command::apply(self, command);
        if moved {
            self.drawing.reread();
            // Anything that is gone can no longer be selected.
            let alive: FxHashSet<Id> = self
                .drawing
                .elements
                .iter()
                .filter(|element| !element.is_deleted)
                .map(|element| element.id.clone())
                .collect();
            self.selection.retain(|id| alive.contains(id));
        }
        moved
    }

    /// The drawing as a file.
    ///
    /// # Errors
    ///
    /// If the document holds something that cannot be written as JSON, which one read from JSON
    /// never does.
    pub fn to_string(&self) -> Result<String, serde_json::Error> {
        self.drawing.to_string()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A scene over `elements`, with the clock and the generator held still.
    pub(crate) fn scene(elements: &str) -> Scene {
        let text = format!(r#"{{"type":"excalidraw","version":2,"elements":{elements}}}"#);
        let drawing = crate::file::parse(&text).expect("a drawing");
        Scene::new(drawing, 1, 1_756_304_871_234)
    }

    #[test]
    fn selecting_one_member_of_a_group_selects_the_group() {
        let mut scene = scene(
            r#"[{"type":"rectangle","id":"a","groupIds":["g"]},
                {"type":"rectangle","id":"b","groupIds":["g"]},
                {"type":"rectangle","id":"c"}]"#,
        );
        scene.select([Id::new("a")]);
        assert_eq!(scene.selection().len(), 2);
        assert!(scene.is_selected(&Id::new("b")));
        assert!(!scene.is_selected(&Id::new("c")));
    }

    #[test]
    fn selecting_something_ungrouped_selects_only_it() {
        let mut scene = scene(r#"[{"type":"rectangle","id":"a"},{"type":"rectangle","id":"b"}]"#);
        scene.select([Id::new("a")]);
        assert_eq!(scene.selection(), [Id::new("a")]);
        scene.clear_selection();
        assert!(!scene.has_selection());
    }

    #[test]
    fn the_selection_has_a_box_around_it() {
        let mut scene = scene(
            r#"[{"type":"rectangle","id":"a","x":0,"y":0,"width":10,"height":10},
                {"type":"rectangle","id":"b","x":100,"y":0,"width":10,"height":10}]"#,
        );
        assert!(scene.selection_bounds().is_none());
        scene.select([Id::new("a"), Id::new("b")]);
        let bounds = scene.selection_bounds().expect("a box");
        assert!((bounds.width() - 110.0).abs() < 1e-9);
    }

    #[test]
    fn fresh_names_differ() {
        let mut scene = scene("[]");
        assert_ne!(scene.fresh_id(), scene.fresh_id());
        assert_ne!(scene.fresh_seed(), scene.fresh_seed());
    }
}
