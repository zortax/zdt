//! What is under the pointer.
//!
//! A press picks the top-most element it lands on. What "lands on" means depends on the element: a
//! filled shape is hit anywhere inside it, an unfilled one only near its outline, and a line only
//! near the line itself. The tolerance is in scene units, so a caller working in screen pixels
//! divides by the zoom before asking.

use kurbo::{Point, Rect};

use crate::element::{Data, Element, Kind};
use crate::geom::{self, Placement, outline};

/// How near an outline a press has to be to count, before the zoom.
pub const TOLERANCE: f64 = 10.0;

/// Whether `at`, in the scene, lands on `element`.
#[must_use]
pub fn hits(element: &Element, at: Point, tolerance: f64) -> bool {
    if element.is_deleted || element.kind == Kind::Selection {
        return false;
    }
    let placement = Placement::of(element);
    let local = placement.local(at);
    let reach = tolerance.max(element.stroke_width / 2.0);

    match element.kind {
        // A frame is taken hold of by its edge, so a press inside it reaches what is in it.
        Kind::Frame | Kind::Magicframe => near_outline(element, local, reach),
        // Words and pictures are solid: anywhere in the box is on them.
        Kind::Text | Kind::Image | Kind::Embeddable | Kind::Iframe => {
            box_of(element).inflate(reach, reach).contains(local)
        }
        Kind::Freedraw => near_stroke(element, local, reach),
        Kind::Line | Kind::Arrow => near_linear(element, local, reach),
        _ if element.is_filled() => inside(element, local) || near_outline(element, local, reach),
        _ => near_outline(element, local, reach),
    }
}

/// The box an element takes in its own space.
fn box_of(element: &Element) -> Rect {
    Rect::new(0.0, 0.0, element.width, element.height)
}

/// Whether `at` is inside the element's outline.
fn inside(element: &Element, at: Point) -> bool {
    geom::inside_polygon(at, &outline::as_points(element))
}

/// Whether `at` is within `reach` of the element's outline.
fn near_outline(element: &Element, at: Point, reach: f64) -> bool {
    let points = outline::as_points(element);
    near_run(at, &points, reach, true)
}

/// Whether `at` is within `reach` of the line's own run of points.
fn near_linear(element: &Element, at: Point, reach: f64) -> bool {
    let Data::Linear(linear) = &element.data else {
        return false;
    };
    // A closed, filled line is a shape, so its inside counts too.
    if linear.polygon && element.is_filled() && geom::inside_polygon(at, &linear.points) {
        return true;
    }
    // The line as it is drawn, so a round line is hit where the curve is rather than where the
    // points behind it are.
    let points = flattened(element);
    near_run(at, &points, reach, linear.polygon)
}

/// Whether `at` is on the ring a pen stroke fills.
fn near_stroke(element: &Element, at: Point, reach: f64) -> bool {
    let ring = outline::of(element);
    if ring.is_empty() {
        return false;
    }
    if geom::inside_polygon(at, &flattened(element)) {
        return true;
    }
    // A stroke drawn thin is hard to land on, so its edge is worth the same tolerance as anything
    // else.
    near_run(at, &flattened(element), reach, true)
}

/// The element's drawn outline as a run of points.
fn flattened(element: &Element) -> Vec<Point> {
    outline::as_points(element)
}

/// Whether `at` is within `reach` of the run `points` makes.
fn near_run(at: Point, points: &[Point], reach: f64, close: bool) -> bool {
    if points.is_empty() {
        return false;
    }
    if points.len() == 1 {
        return (at - points[0]).hypot() <= reach;
    }
    for pair in points.windows(2) {
        if geom::distance_to_segment(at, pair[0], pair[1]) <= reach {
            return true;
        }
    }
    close && geom::distance_to_segment(at, points[points.len() - 1], points[0]) <= reach
}

/// The top-most element `at` lands on.
///
/// The list is in painting order, so the search runs backwards: what was drawn last is what a press
/// takes hold of. Words written inside a shape are part of that shape and are never taken hold of
/// on their own — a press on them is a press on the shape they are in.
#[must_use]
pub fn top_most(elements: &[Element], at: Point, tolerance: f64) -> Option<usize> {
    let found = elements
        .iter()
        .enumerate()
        .rev()
        .find(|(_, element)| !element.locked && hits(element, at, tolerance))
        .map(|(index, _)| index)?;

    let Some(container) = elements[found]
        .text()
        .and_then(|words| words.container_id.as_ref())
    else {
        return Some(found);
    };
    // The shape the words are in, when it is still there.
    elements
        .iter()
        .position(|held| &held.id == container && !held.locked && !held.is_deleted)
        .or(Some(found))
}

/// Every element whose bounds are inside `band`.
///
/// A rubber band takes what it wholly contains, which is what keeps dragging a band across a
/// drawing from picking up everything it brushes.
#[must_use]
pub fn within(elements: &[Element], band: Rect) -> Vec<usize> {
    elements
        .iter()
        .enumerate()
        .filter(|(_, element)| {
            !element.is_deleted && !element.locked && band.contains_rect(geom::bounds::of(element))
        })
        .map(|(index, _)| index)
        .collect()
}

/// Every element whose bounds meet `visible`, which is what a renderer draws.
#[must_use]
pub fn visible(elements: &[Element], visible: Rect) -> Vec<usize> {
    elements
        .iter()
        .enumerate()
        .filter(|(_, element)| {
            !element.is_deleted && {
                // A shape with no area still shows: a hairline is a rectangle of no height.
                let bounds = geom::bounds::of(element);
                bounds.x0 <= visible.x1
                    && bounds.x1 >= visible.x0
                    && bounds.y0 <= visible.y1
                    && bounds.y1 >= visible.y0
            }
        })
        .map(|(index, _)| index)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(json: &str) -> Element {
        let value: serde_json::Value = serde_json::from_str(json).expect("valid JSON");
        crate::element::read(value.as_object().expect("an object")).expect("an element")
    }

    #[test]
    fn an_unfilled_shape_is_only_hit_near_its_outline() {
        let held = read(r#"{"type":"rectangle","x":0,"y":0,"width":100,"height":50}"#);
        assert!(hits(&held, Point::new(0.0, 25.0), 5.0), "on the left edge");
        assert!(!hits(&held, Point::new(50.0, 25.0), 5.0), "in the middle");
    }

    #[test]
    fn a_filled_shape_is_hit_anywhere_inside_it() {
        let held = read(
            r##"{"type":"rectangle","x":0,"y":0,"width":100,"height":50,
                 "backgroundColor":"#a5d8ff"}"##,
        );
        assert!(hits(&held, Point::new(50.0, 25.0), 5.0));
        assert!(!hits(&held, Point::new(200.0, 25.0), 5.0));
    }

    #[test]
    fn a_turned_shape_is_hit_where_it_is_turned_to() {
        let held = read(
            r##"{"type":"rectangle","x":0,"y":0,"width":100,"height":20,
                 "backgroundColor":"#a5d8ff","angle":1.5707963268}"##,
        );
        // Standing on its end, the middle of the box is still inside it.
        assert!(hits(&held, Point::new(50.0, 10.0), 2.0));
        // And a point that would be inside it lying down is not.
        assert!(!hits(&held, Point::new(95.0, 10.0), 2.0));
    }

    #[test]
    fn a_line_is_hit_near_the_line_and_nowhere_else() {
        let held = read(r#"{"type":"line","x":0,"y":0,"points":[[0,0],[100,0]]}"#);
        assert!(hits(&held, Point::new(50.0, 2.0), 5.0));
        assert!(!hits(&held, Point::new(50.0, 40.0), 5.0));
    }

    #[test]
    fn a_locked_element_is_never_the_top_most() {
        let one = read(r#"{"type":"rectangle","id":"a","x":0,"y":0,"width":50,"height":50}"#);
        let two = read(
            r#"{"type":"rectangle","id":"b","x":0,"y":0,"width":50,"height":50,"locked":true}"#,
        );
        let found = top_most(&[one, two], Point::new(0.0, 25.0), 5.0);
        assert_eq!(found, Some(0), "the locked one on top is passed over");
    }

    #[test]
    fn the_top_most_is_the_one_drawn_last() {
        let one = read(
            r##"{"type":"rectangle","id":"a","x":0,"y":0,"width":50,"height":50,
                 "backgroundColor":"#a5d8ff"}"##,
        );
        let two = read(
            r##"{"type":"rectangle","id":"b","x":0,"y":0,"width":50,"height":50,
                 "backgroundColor":"#b2f2bb"}"##,
        );
        assert_eq!(top_most(&[one, two], Point::new(25.0, 25.0), 5.0), Some(1));
    }

    #[test]
    fn a_band_takes_what_it_wholly_holds() {
        let inside = read(r#"{"type":"rectangle","id":"a","x":10,"y":10,"width":20,"height":20}"#);
        let across = read(r#"{"type":"rectangle","id":"b","x":40,"y":10,"width":100,"height":20}"#);
        let band = Rect::new(0.0, 0.0, 60.0, 60.0);
        assert_eq!(within(&[inside, across], band), vec![0]);
    }

    #[test]
    fn only_what_is_on_screen_is_drawn() {
        let near = read(r#"{"type":"rectangle","id":"a","x":0,"y":0,"width":20,"height":20}"#);
        let far = read(r#"{"type":"rectangle","id":"b","x":5000,"y":0,"width":20,"height":20}"#);
        let screen = Rect::new(-100.0, -100.0, 400.0, 400.0);
        assert_eq!(visible(&[near, far], screen), vec![0]);
    }

    #[test]
    fn a_press_on_words_inside_a_shape_takes_hold_of_the_shape() {
        let shape = read(
            r##"{"type":"rectangle","id":"box","x":0,"y":0,"width":200,"height":100,
                 "backgroundColor":"#a5d8ff","boundElements":[{"id":"t","type":"text"}]}"##,
        );
        let label = read(
            r#"{"type":"text","id":"t","x":20,"y":40,"width":160,"height":25,"text":"in",
                "containerId":"box"}"#,
        );
        // The words are painted last, so a press lands on them first.
        let found = top_most(&[shape, label], Point::new(100.0, 50.0), 5.0);
        assert_eq!(found, Some(0), "and it is the shape that is taken hold of");
    }

    #[test]
    fn a_press_on_free_words_takes_hold_of_them() {
        let label =
            read(r#"{"type":"text","id":"t","x":0,"y":0,"width":100,"height":25,"text":"free"}"#);
        assert_eq!(top_most(&[label], Point::new(50.0, 12.0), 5.0), Some(0));
    }

    #[test]
    fn a_deleted_element_is_never_hit() {
        let held = read(
            r##"{"type":"rectangle","x":0,"y":0,"width":100,"height":50,
                 "backgroundColor":"#a5d8ff","isDeleted":true}"##,
        );
        assert!(!hits(&held, Point::new(50.0, 25.0), 5.0));
    }
}
