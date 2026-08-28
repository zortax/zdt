//! The handles around what is selected.
//!
//! The box and its handles are drawn and hit in view pixels, so they keep their size at every zoom.
//! A single element's box is turned with the element; a box around several is upright, because
//! there is no one angle several elements share.

use excalidraw::Element;
use excalidraw::geom::Placement;
use kurbo::{Point, Rect, Vec2};

use crate::state::Sides;
use crate::viewport::Viewport;

/// How wide one handle is drawn, in view pixels.
pub const SIZE: f64 = 8.0;
/// How near one a press has to be to take hold of it.
pub const GRIP: f64 = 8.0;
/// How far the turning handle sits above the box.
pub const ROTATION_GAP: f64 = 16.0;
/// How far the box is drawn outside what it holds.
pub const MARGIN: f64 = 4.0;
/// How large a box has to be before it grows handles on its sides.
pub const SIDE_THRESHOLD: f64 = 40.0;

/// Which handle a press took hold of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grip {
    /// One that scales.
    Scale(Sides),
    /// The one that turns.
    Rotate,
}

/// The box around what is selected, in view pixels, and how it is turned.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Frame {
    /// The box, upright, before the turn.
    pub box_: Rect,
    /// How far it is turned, in radians.
    pub angle: f64,
    /// What it turns about.
    ///
    /// The element's own middle, which is not the middle of this box: a pen stroke's ink reaches
    /// further one way than the other, and it still turns about the middle of the box it was
    /// written with.
    pub about: Point,
}

impl Frame {
    /// The frame around `selected`, when anything is.
    ///
    /// One element's frame is its own box, turned with it. A frame around several is upright and
    /// holds all of them.
    #[must_use]
    pub fn of(selected: &[&Element], viewport: &Viewport) -> Option<Self> {
        match selected {
            [] => None,
            [only] => {
                let placement = Placement::of(only);
                // The box of what is actually drawn, which for a pen stroke or a line is wider
                // than the points it was drawn from.
                let local = excalidraw::geom::bounds::local(only);
                let zoom = viewport.zoom();
                let origin = viewport.place(placement.box_.origin() + local.origin().to_vec2());
                Some(Self {
                    box_: Rect::new(
                        origin.x,
                        origin.y,
                        origin.x + local.width() * zoom,
                        origin.y + local.height() * zoom,
                    ),
                    angle: only.angle,
                    about: viewport.place(placement.center()),
                })
            }
            many => {
                let bounds = excalidraw::geom::of_many(many.iter().copied())?;
                let top_left = viewport.place(bounds.origin());
                let zoom = viewport.zoom();
                let box_ = Rect::new(
                    top_left.x,
                    top_left.y,
                    top_left.x + bounds.width() * zoom,
                    top_left.y + bounds.height() * zoom,
                );
                Some(Self {
                    box_,
                    angle: 0.0,
                    about: box_.center(),
                })
            }
        }
    }

    /// The box, with the margin the handles sit on.
    #[must_use]
    pub fn outline(&self) -> Rect {
        self.box_.inflate(MARGIN, MARGIN)
    }

    /// What the frame turns about.
    #[must_use]
    pub const fn center(&self) -> Point {
        self.about
    }

    /// `at` in the view, in the frame's own upright space.
    #[must_use]
    pub fn local(&self, at: Point) -> Point {
        excalidraw::geom::rotated(at, self.center(), -self.angle)
    }

    /// `at` in that space, back in the view.
    #[must_use]
    pub fn screen(&self, at: Point) -> Point {
        excalidraw::geom::rotated(at, self.center(), self.angle)
    }

    /// Every handle, with the edges each one moves, in the view.
    #[must_use]
    pub fn scale_handles(&self) -> Vec<(Sides, Point)> {
        let box_ = self.outline();
        let side = |left, right, top, bottom| Sides {
            left,
            right,
            top,
            bottom,
        };
        let middle = box_.center();
        let mut held = vec![
            (side(true, false, true, false), Point::new(box_.x0, box_.y0)),
            (side(false, true, true, false), Point::new(box_.x1, box_.y0)),
            (side(true, false, false, true), Point::new(box_.x0, box_.y1)),
            (side(false, true, false, true), Point::new(box_.x1, box_.y1)),
        ];
        // A box too small for them would have its side handles on top of its corner ones.
        if box_.width() > SIDE_THRESHOLD {
            held.push((
                side(false, false, true, false),
                Point::new(middle.x, box_.y0),
            ));
            held.push((
                side(false, false, false, true),
                Point::new(middle.x, box_.y1),
            ));
        }
        if box_.height() > SIDE_THRESHOLD {
            held.push((
                side(true, false, false, false),
                Point::new(box_.x0, middle.y),
            ));
            held.push((
                side(false, true, false, false),
                Point::new(box_.x1, middle.y),
            ));
        }
        held.into_iter()
            .map(|(sides, at)| (sides, self.screen(at)))
            .collect()
    }

    /// Where the turning handle sits, in the view.
    #[must_use]
    pub fn rotation_handle(&self) -> Point {
        let box_ = self.outline();
        self.screen(Point::new(box_.center().x, box_.y0 - ROTATION_GAP))
    }

    /// Which handle `at` takes hold of, when it takes hold of one.
    #[must_use]
    pub fn grip(&self, at: Point) -> Option<Grip> {
        if (self.rotation_handle() - at).hypot() <= GRIP {
            return Some(Grip::Rotate);
        }
        self.scale_handles()
            .into_iter()
            .find(|(_, handle)| (*handle - at).hypot() <= GRIP)
            .map(|(sides, _)| Grip::Scale(sides))
    }

    /// Whether `at` is inside the frame, which is what a drag takes hold of.
    #[must_use]
    pub fn holds(&self, at: Point) -> bool {
        self.outline().contains(self.local(at))
    }
}

/// The box `from` becomes when `sides` are dragged by `by`.
///
/// The box never turns inside out: dragging an edge past the opposite one gives the box between
/// them, which is what makes flipping a shape by dragging work.
#[must_use]
pub fn resized(from: Rect, sides: Sides, by: Vec2, keep_ratio: bool, from_center: bool) -> Rect {
    let mut by = by;
    if keep_ratio && from.width() > 0.0 && from.height() > 0.0 {
        // The larger of the two, taken on both axes in the ratio the box already has.
        let ratio = from.height() / from.width();
        if (by.x * ratio).abs() > by.y.abs() {
            by.y = by.x * ratio * if sides.top == sides.left { 1.0 } else { -1.0 };
        } else {
            by.x = by.y / ratio * if sides.top == sides.left { 1.0 } else { -1.0 };
        }
    }

    let mut next = from;
    if sides.left {
        next.x0 += by.x;
        if from_center {
            next.x1 -= by.x;
        }
    }
    if sides.right {
        next.x1 += by.x;
        if from_center {
            next.x0 -= by.x;
        }
    }
    if sides.top {
        next.y0 += by.y;
        if from_center {
            next.y1 -= by.y;
        }
    }
    if sides.bottom {
        next.y1 += by.y;
        if from_center {
            next.y0 -= by.y;
        }
    }
    next.abs()
}

/// How far a turn has come, from where it started to where the pointer is.
#[must_use]
pub fn turn(about: Point, from: Point, to: Point, snap: bool) -> f64 {
    let angle = |at: Point| (at.y - about.y).atan2(at.x - about.x);
    let mut held = angle(to) - angle(from);
    if snap {
        // Fifteen degrees, which is what shift asks for.
        let step = std::f64::consts::PI / 12.0;
        held = (held / step).round() * step;
    }
    held
}

/// How wide a line's own point handles are drawn, in view pixels.
pub const POINT_SIZE: f64 = 7.0;

/// And the ones between them, which are not points yet.
pub const MIDPOINT_SIZE: f64 = 6.0;

/// One handle on a selected line.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PointHandle {
    /// Where it is, in view pixels.
    pub at: Point,
    /// Which point it moves, once it is one.
    pub index: usize,
    /// Whether it is a point already, or the middle of a segment that becomes one when dragged.
    pub real: bool,
}

/// Every handle a selected line offers, in view pixels.
///
/// Its own points, and the middle of each segment between them. Dragging a middle makes it a
/// point, and the two halves it leaves behind get middles of their own — which is how a straight
/// line becomes a curve one bend at a time.
#[must_use]
pub fn point_handles(element: &excalidraw::Element, viewport: &Viewport) -> Vec<PointHandle> {
    let Some(linear) = element.linear() else {
        return Vec::new();
    };
    if linear.points.len() < 2 {
        return Vec::new();
    }
    let placement = Placement::of(element);
    let scene = |at: Point| viewport.place(placement.scene(at));

    let mut held = Vec::with_capacity(linear.points.len() * 2);
    for (index, point) in linear.points.iter().enumerate() {
        held.push(PointHandle {
            at: scene(*point),
            index,
            real: true,
        });
        if let Some(next) = linear.points.get(index + 1) {
            let middle = Point::new((point.x + next.x) / 2.0, (point.y + next.y) / 2.0);
            held.push(PointHandle {
                at: scene(middle),
                // Dragging it puts a point here, between this one and the next.
                index: index + 1,
                real: false,
            });
        }
    }
    held
}

/// Which of a line's handles `at` takes hold of.
///
/// A real point wins over a middle: they never overlap on a line with room to bend, and on one
/// without it the point is what the reader means.
#[must_use]
pub fn grip_point(handles: &[PointHandle], at: Point) -> Option<PointHandle> {
    let near = |real: bool| {
        handles
            .iter()
            .filter(|held| held.real == real)
            .find(|held| (held.at - at).hypot() <= GRIP)
            .copied()
    };
    near(true).or_else(|| near(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(json: &str) -> Element {
        let value: serde_json::Value = serde_json::from_str(json).expect("valid JSON");
        excalidraw::element::read(value.as_object().expect("an object")).expect("an element")
    }

    fn viewport() -> Viewport {
        let viewport = Viewport::new();
        viewport.set_size(800.0, 600.0);
        viewport
    }

    #[test]
    fn one_element_gets_a_frame_the_size_of_its_box() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let held =
                read(r#"{"type":"rectangle","id":"a","x":10,"y":20,"width":100,"height":50}"#);
            let frame = Frame::of(&[&held], &viewport()).expect("a frame");
            assert!((frame.box_.width() - 100.0).abs() < 1e-9);
            assert!((frame.box_.x0 - 10.0).abs() < 1e-9);
            assert!(frame.angle.abs() < f64::EPSILON);
        });
    }

    /// A pen stroke is wider than the points it was drawn from — the ink spreads either side of
    /// them — so a box drawn around those points has the stroke poking out of it.
    #[test]
    fn a_frame_holds_the_whole_of_a_hand_drawn_stroke() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let held = read(
                r#"{"type":"freedraw","x":100,"y":100,"points":[[0,0],[60,20],[120,0]],
                    "strokeWidth":4,"simulatePressure":true}"#,
            );
            let viewport = viewport();
            let frame = Frame::of(&[&held], &viewport).expect("a frame");

            // Every corner of what is actually drawn is inside the box, with the margin to spare.
            let drawn = excalidraw::geom::bounds::of(&held);
            let corner = |x: f64, y: f64| viewport.place(Point::new(x, y));
            let box_ = frame.outline();
            for (x, y) in [
                (drawn.x0, drawn.y0),
                (drawn.x1, drawn.y0),
                (drawn.x1, drawn.y1),
                (drawn.x0, drawn.y1),
            ] {
                let at = corner(x, y);
                assert!(box_.contains(at), "{at:?} is outside the frame {box_:?}");
            }
        });
    }

    /// And it is not needlessly large: the box still hugs what is drawn.
    #[test]
    fn a_frame_is_no_bigger_than_what_it_holds_needs() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let held = read(
                r#"{"type":"freedraw","x":100,"y":100,"points":[[0,0],[60,20],[120,0]],
                    "strokeWidth":4,"simulatePressure":true}"#,
            );
            let frame = Frame::of(&[&held], &viewport()).expect("a frame");
            let drawn = excalidraw::geom::bounds::of(&held);
            assert!(
                frame.box_.width() < drawn.width() + 4.0,
                "the frame is {} wide against the {} it holds",
                frame.box_.width(),
                drawn.width()
            );
        });
    }

    #[test]
    fn a_frame_around_several_is_upright_and_holds_them_all() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let one = read(
                r#"{"type":"rectangle","id":"a","x":0,"y":0,"width":10,"height":10,
                    "angle":0.7853981634}"#,
            );
            let two = read(r#"{"type":"rectangle","id":"b","x":100,"y":0,"width":10,"height":10}"#);
            let frame = Frame::of(&[&one, &two], &viewport()).expect("a frame");
            assert!(frame.angle.abs() < f64::EPSILON);
            assert!(frame.box_.width() > 110.0);
        });
    }

    #[test]
    fn nothing_selected_has_no_frame() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            assert!(Frame::of(&[], &viewport()).is_none());
        });
    }

    #[test]
    fn a_small_box_grows_only_its_corner_handles() {
        let frame = Frame {
            box_: Rect::new(0.0, 0.0, 20.0, 20.0),
            angle: 0.0,
            about: Rect::new(0.0, 0.0, 20.0, 20.0).center(),
        };
        assert_eq!(frame.scale_handles().len(), 4);
        let large = Frame {
            box_: Rect::new(0.0, 0.0, 200.0, 200.0),
            angle: 0.0,
            about: Rect::new(0.0, 0.0, 200.0, 200.0).center(),
        };
        assert_eq!(large.scale_handles().len(), 8);
    }

    #[test]
    fn a_press_on_a_handle_takes_hold_of_it() {
        let frame = Frame {
            box_: Rect::new(0.0, 0.0, 200.0, 100.0),
            angle: 0.0,
            about: Rect::new(0.0, 0.0, 200.0, 100.0).center(),
        };
        let (sides, at) = frame.scale_handles()[0];
        assert_eq!(frame.grip(at), Some(Grip::Scale(sides)));
        assert_eq!(frame.grip(frame.rotation_handle()), Some(Grip::Rotate));
        assert!(frame.grip(Point::new(100.0, 50.0)).is_none());
    }

    #[test]
    fn a_turned_frames_handles_turn_with_it() {
        let frame = Frame {
            box_: Rect::new(0.0, 0.0, 100.0, 100.0),
            angle: std::f64::consts::FRAC_PI_2,
            about: Rect::new(0.0, 0.0, 100.0, 100.0).center(),
        };
        let handles = frame.scale_handles();
        // The handle that was at the top left is now at the top right.
        let corner = handles[0].1;
        assert!(corner.x > 50.0, "it turned round the middle");
        assert_eq!(frame.grip(corner), Some(Grip::Scale(handles[0].0)));
    }

    #[test]
    fn dragging_an_edge_moves_only_that_edge() {
        let from = Rect::new(0.0, 0.0, 100.0, 100.0);
        let sides = Sides {
            left: false,
            right: true,
            top: false,
            bottom: false,
        };
        let next = resized(from, sides, Vec2::new(50.0, 30.0), false, false);
        assert!((next.x1 - 150.0).abs() < 1e-9);
        assert!((next.y1 - 100.0).abs() < 1e-9);
    }

    #[test]
    fn dragging_an_edge_past_its_opposite_flips_the_box() {
        let from = Rect::new(0.0, 0.0, 100.0, 100.0);
        let sides = Sides {
            left: false,
            right: true,
            top: false,
            bottom: false,
        };
        let next = resized(from, sides, Vec2::new(-150.0, 0.0), false, false);
        assert!(next.width() > 0.0, "the box is the one between the edges");
        assert!((next.x1 - 0.0).abs() < 1e-9);
    }

    #[test]
    fn alt_scales_about_the_middle() {
        let from = Rect::new(0.0, 0.0, 100.0, 100.0);
        let sides = Sides {
            left: false,
            right: true,
            top: false,
            bottom: false,
        };
        let next = resized(from, sides, Vec2::new(20.0, 0.0), false, true);
        assert!((next.center().x - from.center().x).abs() < 1e-9);
        assert!((next.width() - 140.0).abs() < 1e-9);
    }

    #[test]
    fn shift_keeps_the_ratio() {
        let from = Rect::new(0.0, 0.0, 100.0, 50.0);
        let sides = Sides {
            left: false,
            right: true,
            top: false,
            bottom: true,
        };
        let next = resized(from, sides, Vec2::new(100.0, 0.0), true, false);
        assert!((next.width() / next.height() - 2.0).abs() < 1e-6);
    }

    #[test]
    fn a_turn_is_measured_from_where_it_started() {
        let about = Point::ZERO;
        let held = turn(about, Point::new(10.0, 0.0), Point::new(0.0, 10.0), false);
        assert!((held - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    }

    #[test]
    fn shift_snaps_a_turn_to_fifteen_degrees() {
        let about = Point::ZERO;
        let held = turn(about, Point::new(10.0, 0.0), Point::new(10.0, 1.0), true);
        assert!(held.abs() < 1e-9, "a small turn snaps back to none");
        let quarter = turn(about, Point::new(10.0, 0.0), Point::new(0.4, 10.0), true);
        let step = std::f64::consts::PI / 12.0;
        assert!((quarter / step - (quarter / step).round()).abs() < 1e-9);
    }
}
