//! Where an element is, and what shape it makes there.
//!
//! Two spaces matter. An element's own space has its unrotated box starting at the origin; the
//! scene's has it at `x, y` and turned by `angle` about the middle of that box. Everything drawn,
//! hit or selected goes through [`Placement`], which is the one mapping between them.

pub mod arrowhead;
pub mod binding;
pub mod bounds;
pub mod diamond;
pub mod elbow;
pub mod outline;

use kurbo::{Affine, Point, Rect, Vec2};

use crate::element::Element;

pub use self::bounds::{Bounds, of_many};
pub use self::diamond::diamond_points;

/// Where an element sits in the scene.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Placement {
    /// The unrotated box, in the scene.
    pub box_: Rect,
    /// How far it is turned about the middle of that box, in radians.
    pub angle: f64,
}

impl Placement {
    /// Where `element` sits.
    #[must_use]
    pub fn of(element: &Element) -> Self {
        Self {
            box_: Rect::new(
                element.x,
                element.y,
                element.x + element.width,
                element.y + element.height,
            ),
            angle: element.angle,
        }
    }

    /// The middle of the box, which is what a rotation turns about.
    #[must_use]
    pub fn center(&self) -> Point {
        self.box_.center()
    }

    /// The element's own space onto the scene's.
    #[must_use]
    pub fn to_scene(&self) -> Affine {
        let center = self.center();
        Affine::translate(center.to_vec2())
            * Affine::rotate(self.angle)
            * Affine::translate(-center.to_vec2())
            * Affine::translate(self.box_.origin().to_vec2())
    }

    /// The scene's space onto the element's.
    #[must_use]
    pub fn to_local(&self) -> Affine {
        self.to_scene().inverse()
    }

    /// `at` in the scene, taken to the element's own space.
    #[must_use]
    pub fn local(&self, at: Point) -> Point {
        self.to_local() * at
    }

    /// `at` in the element's space, taken to the scene's.
    #[must_use]
    pub fn scene(&self, at: Point) -> Point {
        self.to_scene() * at
    }

    /// The four corners of the box, turned, in the scene.
    #[must_use]
    pub fn corners(&self) -> [Point; 4] {
        let to_scene = self.to_scene();
        let (w, h) = (self.box_.width(), self.box_.height());
        [
            to_scene * Point::new(0.0, 0.0),
            to_scene * Point::new(w, 0.0),
            to_scene * Point::new(w, h),
            to_scene * Point::new(0.0, h),
        ]
    }
}

/// `at` turned `angle` about `center`.
#[must_use]
pub fn rotated(at: Point, center: Point, angle: f64) -> Point {
    let (sin, cos) = angle.sin_cos();
    let d = at - center;
    Point::new(
        d.x * cos - d.y * sin + center.x,
        d.x * sin + d.y * cos + center.y,
    )
}

/// How far `at` is from the segment between `from` and `to`.
#[must_use]
pub fn distance_to_segment(at: Point, from: Point, to: Point) -> f64 {
    let along = to - from;
    let length_sq = along.hypot2();
    if length_sq == 0.0 {
        return (at - from).hypot();
    }
    let t = ((at - from).dot(along) / length_sq).clamp(0.0, 1.0);
    (at - (from + along * t)).hypot()
}

/// Whether `at` is inside the ring `points` makes.
#[must_use]
pub fn inside_polygon(at: Point, points: &[Point]) -> bool {
    if points.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut previous = points[points.len() - 1];
    for point in points {
        // A ray to the right, counting the edges it crosses.
        if (point.y > at.y) != (previous.y > at.y) {
            let crossing =
                (previous.x - point.x) * (at.y - point.y) / (previous.y - point.y) + point.x;
            if at.x < crossing {
                inside = !inside;
            }
        }
        previous = *point;
    }
    inside
}

/// `vector`, one unit long. A vector of no length stays as it is.
#[must_use]
pub fn unit(vector: Vec2) -> Vec2 {
    let length = vector.hypot();
    if length == 0.0 {
        vector
    } else {
        vector / length
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::{Data, Kind, Seed};

    fn element(x: f64, y: f64, width: f64, height: f64, angle: f64) -> Element {
        Element {
            id: "a".into(),
            kind: Kind::Rectangle,
            x,
            y,
            width,
            height,
            angle,
            stroke_color: "#000000".to_owned(),
            background_color: "transparent".to_owned(),
            fill_style: crate::element::FillStyle::Solid,
            stroke_width: 2.0,
            stroke_style: crate::element::StrokeStyle::Solid,
            roughness: 1.0,
            opacity: 100.0,
            group_ids: Vec::new(),
            frame_id: None,
            roundness: None,
            seed: Seed(1),
            version: 1,
            version_nonce: 0,
            index: None,
            is_deleted: false,
            bound_elements: Vec::new(),
            updated: 0,
            link: None,
            locked: false,
            data: Data::Shape,
        }
    }

    #[test]
    fn an_unturned_element_maps_its_own_origin_to_its_corner() {
        let placement = Placement::of(&element(10.0, 20.0, 100.0, 50.0, 0.0));
        assert_eq!(placement.scene(Point::ZERO), Point::new(10.0, 20.0));
        assert_eq!(
            placement.local(Point::new(110.0, 70.0)),
            Point::new(100.0, 50.0)
        );
    }

    #[test]
    fn a_turned_element_turns_about_the_middle_of_its_box() {
        let placement = Placement::of(&element(
            0.0,
            0.0,
            100.0,
            100.0,
            std::f64::consts::FRAC_PI_2,
        ));
        // A quarter turn takes the top left corner to the top right.
        let corner = placement.scene(Point::ZERO);
        assert!((corner.x - 100.0).abs() < 1e-9);
        assert!(corner.y.abs() < 1e-9);
        // And the middle stays put.
        let middle = placement.scene(Point::new(50.0, 50.0));
        assert!((middle - Point::new(50.0, 50.0)).hypot() < 1e-9);
    }

    #[test]
    fn a_point_round_trips_through_both_spaces() {
        let placement = Placement::of(&element(30.0, 40.0, 80.0, 20.0, 0.7));
        let at = Point::new(17.0, 9.0);
        let back = placement.local(placement.scene(at));
        assert!((back - at).hypot() < 1e-9);
    }

    #[test]
    fn a_point_inside_a_ring_is_inside_it() {
        let square = [
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(0.0, 10.0),
        ];
        assert!(inside_polygon(Point::new(5.0, 5.0), &square));
        assert!(!inside_polygon(Point::new(15.0, 5.0), &square));
        assert!(!inside_polygon(Point::new(5.0, 5.0), &square[..2]));
    }

    #[test]
    fn the_distance_to_a_segment_is_measured_to_its_ends_past_them() {
        let (from, to) = (Point::new(0.0, 0.0), Point::new(10.0, 0.0));
        assert!((distance_to_segment(Point::new(5.0, 3.0), from, to) - 3.0).abs() < 1e-9);
        assert!((distance_to_segment(Point::new(-4.0, 0.0), from, to) - 4.0).abs() < 1e-9);
        assert!((distance_to_segment(Point::new(0.0, 0.0), from, from) - 0.0).abs() < 1e-9);
    }
}
