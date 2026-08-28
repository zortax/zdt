//! How much room an element takes in the scene.
//!
//! A boxed element's bounds are the corners of its box, turned. A line's are the curve it is
//! actually drawn as, not the points it was drawn from — a round line bulges past its points, and a
//! selection box that ignored that would not hold the line it is around.

use kurbo::{Point, Rect, Shape as _};

use crate::element::{Element, Kind};

use super::{Placement, outline};

/// The rectangle an element takes in the scene.
pub type Bounds = Rect;

/// The bounds of `element`.
#[must_use]
pub fn of(element: &Element) -> Bounds {
    let placement = Placement::of(element);
    if element.kind.is_linear() || element.kind == Kind::Freedraw {
        // The shape it is drawn as, taken to the scene.
        let mut path = outline::of(element);
        path.apply_affine(placement.to_scene());
        let bounds = path.bounding_box();
        if bounds.width().is_finite() && bounds.height().is_finite() && !path.is_empty() {
            return bounds;
        }
    }
    let corners = placement.corners();
    let mut bounds = Rect::from_points(corners[0], corners[1]);
    for corner in &corners[2..] {
        bounds = bounds.union_pt(*corner);
    }
    bounds
}

/// The rectangle a set of elements takes together.
#[must_use]
pub fn of_many<'a>(elements: impl IntoIterator<Item = &'a Element>) -> Option<Bounds> {
    let mut held: Option<Bounds> = None;
    for element in elements {
        let bounds = of(element);
        held = Some(match held {
            Some(so_far) => so_far.union(bounds),
            None => bounds,
        });
    }
    held
}

/// The box an element takes in its own space, before it is turned.
///
/// For most kinds that is simply its width and height. A pen stroke and a line are wider than the
/// points they were drawn from — a stroke by half its own width, a curve by however far it bulges —
/// so for those it is the box of what is actually drawn. That is what a selection has to be drawn
/// around, or the line pokes out of it.
#[must_use]
pub fn local(element: &Element) -> Bounds {
    let box_ = Rect::new(0.0, 0.0, element.width, element.height);
    if !(element.kind.is_linear() || element.kind == Kind::Freedraw) {
        return box_;
    }
    let drawn = outline::of(element);
    if drawn.is_empty() {
        return box_;
    }
    let bounds = drawn.bounding_box();
    if bounds.width().is_finite() && bounds.height().is_finite() {
        // Never smaller than the points it was drawn from.
        bounds.union(box_)
    } else {
        box_
    }
}

/// The middle of an element, which is what a rotation turns about.
#[must_use]
pub fn center(element: &Element) -> Point {
    Placement::of(element).center()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(json: &str) -> Element {
        let value: serde_json::Value = serde_json::from_str(json).expect("valid JSON");
        crate::element::read(value.as_object().expect("an object")).expect("an element")
    }

    #[test]
    fn an_unturned_box_is_its_own_bounds() {
        let held = read(r#"{"type":"rectangle","x":10,"y":20,"width":100,"height":50}"#);
        let bounds = of(&held);
        assert!((bounds.x0 - 10.0).abs() < 1e-9);
        assert!((bounds.x1 - 110.0).abs() < 1e-9);
        assert!((bounds.y1 - 70.0).abs() < 1e-9);
    }

    /// A quarter turn swaps the two sides; anything between them reaches past both.
    #[test]
    fn a_turned_box_is_measured_where_it_is_drawn() {
        let straight = read(r#"{"type":"rectangle","x":0,"y":0,"width":100,"height":20}"#);
        let quarter = read(
            r#"{"type":"rectangle","x":0,"y":0,"width":100,"height":20,"angle":1.5707963268}"#,
        );
        assert!((of(&quarter).width() - of(&straight).height()).abs() < 1e-6);
        assert!((of(&quarter).height() - of(&straight).width()).abs() < 1e-6);

        let eighth = read(
            r#"{"type":"rectangle","x":0,"y":0,"width":100,"height":20,"angle":0.7853981634}"#,
        );
        assert!(of(&eighth).height() > of(&straight).height());
    }

    #[test]
    fn a_round_line_bulges_past_the_points_it_was_drawn_from() {
        let sharp = read(r#"{"type":"line","points":[[0,0],[50,-40],[100,0]]}"#);
        let round =
            read(r#"{"type":"line","points":[[0,0],[50,-40],[100,0]],"roundness":{"type":2}}"#);
        assert!(
            of(&round).height() > of(&sharp).height(),
            "the curve reaches past the middle point"
        );
    }

    #[test]
    fn a_pen_strokes_bounds_hold_the_whole_stroke() {
        let held = read(
            r#"{"type":"freedraw","x":10,"y":10,"points":[[0,0],[20,4],[40,0]],
                "strokeWidth":2,"simulatePressure":true}"#,
        );
        let bounds = of(&held);
        assert!(
            bounds.x0 < 10.0,
            "the ring reaches back past the first point"
        );
        assert!(bounds.x1 > 50.0);
    }

    #[test]
    fn a_pen_strokes_own_box_holds_the_whole_stroke() {
        let held = read(
            r#"{"type":"freedraw","x":0,"y":0,"points":[[0,0],[40,0]],
                "strokeWidth":4,"simulatePressure":true}"#,
        );
        let box_ = local(&held);
        assert!(
            box_.x0 < 0.0 && box_.y0 < 0.0,
            "the ink reaches back past the first point: {box_:?}"
        );
        assert!(box_.width() > held.width, "and out past the last");
    }

    #[test]
    fn a_shapes_own_box_is_its_width_and_height() {
        let held = read(r#"{"type":"rectangle","x":10,"y":20,"width":100,"height":50}"#);
        let box_ = local(&held);
        assert!((box_.x0).abs() < f64::EPSILON && (box_.y0).abs() < f64::EPSILON);
        assert!((box_.width() - 100.0).abs() < f64::EPSILON);
        assert!((box_.height() - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn several_elements_are_held_by_one_rectangle() {
        let one = read(r#"{"type":"rectangle","x":0,"y":0,"width":10,"height":10}"#);
        let two = read(r#"{"type":"rectangle","x":100,"y":50,"width":10,"height":10}"#);
        let bounds = of_many([&one, &two]).expect("two elements");
        assert!((bounds.x0).abs() < 1e-9);
        assert!((bounds.x1 - 110.0).abs() < 1e-9);
        assert!((bounds.y1 - 60.0).abs() < 1e-9);
        assert!(of_many(std::iter::empty()).is_none());
    }
}
