//! The four points a diamond is drawn through.
//!
//! The top and right points are a pixel past the middle, which is what Excalidraw does so that a
//! diamond of an odd width still has area for the drawing library to work with.

use kurbo::Point;

/// The top, right, bottom and left points of a diamond `width` by `height`, in its own space.
#[must_use]
pub fn diamond_points(width: f64, height: f64) -> [Point; 4] {
    let top_x = (width / 2.0).floor() + 1.0;
    let right_y = (height / 2.0).floor() + 1.0;
    [
        Point::new(top_x, 0.0),
        Point::new(width, right_y),
        Point::new(top_x, height),
        Point::new(0.0, right_y),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_diamond_reaches_every_side_of_its_box() {
        let [top, right, bottom, left] = diamond_points(220.0, 128.0);
        assert!((top.y - 0.0).abs() < f64::EPSILON);
        assert!((right.x - 220.0).abs() < f64::EPSILON);
        assert!((bottom.y - 128.0).abs() < f64::EPSILON);
        assert!((left.x - 0.0).abs() < f64::EPSILON);
        assert_eq!(top.x, bottom.x);
        assert_eq!(right.y, left.y);
    }

    #[test]
    fn an_odd_diamond_still_has_area() {
        let [top, right, ..] = diamond_points(1.0, 1.0);
        assert!(top.x > 0.0 && right.y > 0.0);
    }
}
