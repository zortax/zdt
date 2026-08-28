//! Drawing the pictures in a drawing.
//!
//! A canvas cannot draw a picture either, so each image element is an ordinary image element,
//! placed and turned by the view. The bytes come out of the file, are decoded once, and are held
//! for as long as the drawing shows them.

use std::cell::RefCell;
use std::rc::Rc;

use excalidraw::{Element, Kind};
use kurbo::Point;
use rustc_hash::FxHashMap;

use crate::viewport::Viewport;

/// Where a picture is drawn, in view pixels.
#[derive(Clone, PartialEq, Debug)]
pub struct Placed {
    /// Where its box begins.
    pub at: Point,
    /// How wide it is drawn.
    pub width: f64,
    /// How tall.
    pub height: f64,
    /// How far it is turned, in degrees.
    pub angle: f64,
    /// How it is flipped, as a sign on each axis.
    pub scale: (f64, f64),
    /// How solid it is.
    pub alpha: f64,
    /// How large its corners are cut, in view pixels.
    pub radius: f64,
    /// Where its bytes are, when they are anywhere.
    pub src: Option<String>,
}

/// Where `element` is drawn, when it is a picture.
#[must_use]
pub fn placed(
    element: &Element,
    src: Option<String>,
    viewport: &Viewport,
    dragged: Option<kurbo::Affine>,
) -> Option<Placed> {
    if element.kind != Kind::Image || element.is_deleted {
        return None;
    }
    let picture = element.image()?;
    let zoom = viewport.zoom();
    let corner = Point::new(element.x, element.y);
    // The drag moves it before it has been written.
    let corner = dragged.map_or(corner, |drag: kurbo::Affine| drag * corner);
    // Tracked, so a picture moves with the view.
    let at = viewport.place(corner);
    Some(Placed {
        at,
        width: element.width * zoom,
        height: element.height * zoom,
        angle: element.angle.to_degrees(),
        scale: picture.scale,
        alpha: element.alpha(),
        radius: excalidraw::geom::outline::corner_radius(
            element,
            element.width.min(element.height),
        ) * zoom,
        src,
    })
}

/// The pictures a drawing holds, decoded once each.
///
/// The bytes cross to the renderer through an in-memory address rather than a file, so a drawing
/// opened from a buffer needs nothing on disk.
#[derive(Clone, Default)]
pub struct Pictures {
    held: Rc<RefCell<FxHashMap<String, Rc<zgui_image::ImageBytes>>>>,
}

impl Pictures {
    /// Nothing decoded yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Where the picture `id` names can be drawn from.
    ///
    /// The first ask decodes it; every later ask for the same picture answers the same address, so
    /// the same picture drawn twice is decoded once.
    pub fn src(&self, files: &excalidraw::file::Files, id: &str) -> Option<String> {
        if let Some(held) = self.held.borrow().get(id) {
            return Some(held.url());
        }
        let bytes = files.get(id)?.bytes()?;
        let held = Rc::new(zgui_image::ImageBytes::new(bytes));
        let url = held.url();
        self.held.borrow_mut().insert(id.to_owned(), held);
        Some(url)
    }

    /// Forgets every picture the drawing no longer names.
    pub fn retain(&self, files: &excalidraw::file::Files) {
        self.held
            .borrow_mut()
            .retain(|id, _| files.get(id).is_some());
    }

    /// How many are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.held.borrow().len()
    }

    /// Whether none are.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.held.borrow().is_empty()
    }
}

/// The transform a picture is drawn with, as a style sheet writes it.
#[must_use]
pub fn transform(placed: &Placed) -> String {
    let mut out = String::new();
    if placed.angle.abs() > f64::EPSILON {
        out.push_str(&format!("rotate({}deg)", placed.angle));
    }
    // A negative scale is a flip, which is what the file means by it.
    if placed.scale.0 < 0.0 || placed.scale.1 < 0.0 {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&format!(
            "scale({}, {})",
            placed.scale.0.signum(),
            placed.scale.1.signum()
        ));
    }
    out
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

    fn files() -> excalidraw::file::Files {
        let mut files = excalidraw::file::Files::default();
        files.insert(
            "abc".to_owned(),
            excalidraw::file::BinaryFile::from_bytes(b"not really a png", "image/png", 1),
        );
        files
    }

    #[test]
    fn a_picture_is_placed_where_it_is_and_scaled_by_the_zoom() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let viewport = viewport();
            viewport.zoom_to(2.0);
            let held = read(
                r#"{"type":"image","id":"a","x":10,"y":20,"width":100,"height":80,
                    "fileId":"abc"}"#,
            );
            let placed = placed(&held, None, &viewport, None).expect("a picture");
            assert!((placed.width - 200.0).abs() < 1e-9);
            assert!((placed.height - 160.0).abs() < 1e-9);
        });
    }

    #[test]
    fn a_flip_is_a_negative_scale() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let held = read(r#"{"type":"image","id":"a","width":10,"height":10,"scale":[1,-1]}"#);
            let placed = placed(&held, None, &viewport(), None).expect("a picture");
            assert!(transform(&placed).contains("scale(1, -1)"));
        });
    }

    #[test]
    fn a_turned_picture_carries_its_angle() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let held =
                read(r#"{"type":"image","id":"a","width":10,"height":10,"angle":1.5707963268}"#);
            let placed = placed(&held, None, &viewport(), None).expect("a picture");
            assert!(transform(&placed).starts_with("rotate(90"));
        });
    }

    #[test]
    fn the_same_picture_is_decoded_once() {
        let pictures = Pictures::new();
        let files = files();
        let first = pictures.src(&files, "abc").expect("an address");
        let second = pictures.src(&files, "abc").expect("an address");
        assert_eq!(first, second);
        assert_eq!(pictures.len(), 1);
    }

    #[test]
    fn a_picture_the_drawing_does_not_hold_has_no_address() {
        let pictures = Pictures::new();
        assert!(pictures.src(&files(), "missing").is_none());
        assert!(pictures.is_empty());
    }

    #[test]
    fn what_the_drawing_no_longer_holds_is_forgotten() {
        let pictures = Pictures::new();
        pictures.src(&files(), "abc");
        assert_eq!(pictures.len(), 1);
        pictures.retain(&excalidraw::file::Files::default());
        assert!(pictures.is_empty());
    }

    #[test]
    fn anything_that_is_not_a_picture_is_not_placed() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let held = read(r#"{"type":"rectangle","id":"a"}"#);
            assert!(placed(&held, None, &viewport(), None).is_none());
        });
    }
}
