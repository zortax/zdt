//! Where the drawing is looked at from.
//!
//! An Excalidraw drawing has no edges, so there is nothing to fit a view to: the camera is a place
//! on an endless plane and a scale. Everything here is in CSS pixels, and the scene's own units are
//! whatever the file wrote.

use kurbo::{Affine, Point, Rect, Vec2};
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

/// The nearest and farthest a zoom can reach.
pub const NEAREST: f64 = 0.1;
/// The farthest.
pub const FARTHEST: f64 = 30.0;
/// How much one zoom step changes the scale.
pub const STEP: f64 = 1.1;
/// How much one wheel notch does.
const WHEEL_STEP: f64 = 1.1;
/// How far one pan key moves the plane, in CSS pixels.
pub const NUDGE: f64 = 48.0;
/// One line of wheel travel, in CSS pixels.
const LINE: f64 = 16.0;

/// One view's place over an endless plane.
///
/// Every field is a signal handle, so the whole of it copies and a key that moves the view can hold
/// one by value.
#[derive(Clone, Copy, PartialEq)]
pub struct Viewport {
    /// The scene point at the view's top left corner.
    scroll: RwSignal<(f64, f64), LocalStorage>,
    /// How many CSS pixels one scene unit is drawn at.
    zoom: RwSignal<f64, LocalStorage>,
    /// How large the view is, from the measured element.
    size: RwSignal<(f64, f64), LocalStorage>,
}

impl Viewport {
    /// Looking at the origin, one to one.
    #[must_use]
    pub fn new() -> Self {
        Self {
            scroll: RwSignal::new_local((0.0, 0.0)),
            zoom: RwSignal::new_local(1.0),
            size: RwSignal::new_local((0.0, 0.0)),
        }
    }

    /// How many CSS pixels one scene unit is drawn at. Tracked.
    #[must_use]
    pub fn zoom(&self) -> f64 {
        self.zoom.get()
    }

    /// The same, without subscribing.
    #[must_use]
    pub fn zoom_untracked(&self) -> f64 {
        self.zoom.get_untracked()
    }

    /// The scene point at the top left corner. Tracked.
    #[must_use]
    pub fn scroll(&self) -> (f64, f64) {
        self.scroll.get()
    }

    /// The same, without subscribing.
    #[must_use]
    pub fn scroll_untracked(&self) -> (f64, f64) {
        self.scroll.get_untracked()
    }

    /// How large the view is. Tracked.
    #[must_use]
    pub fn size(&self) -> (f64, f64) {
        self.size.get()
    }

    /// Says how large it is, from the measured element.
    pub fn set_size(&self, width: f64, height: f64) {
        if self.size.get_untracked() != (width, height) {
            self.size.set((width, height));
        }
    }

    /// The part of the scene the view shows. Tracked.
    #[must_use]
    pub fn visible(&self) -> Rect {
        let (x, y) = self.scroll.get();
        let (width, height) = self.size.get();
        let zoom = self.zoom.get().max(f64::EPSILON);
        Rect::new(x, y, x + width / zoom, y + height / zoom)
    }

    /// The square of scene the canvases are given, which is exactly what is visible. Tracked.
    ///
    /// A canvas fits its view box onto its box uniformly, and this box has the view's own shape, so
    /// the fit is exact and a pan is one property write rather than a redrawing.
    #[must_use]
    pub fn view_box(&self) -> [f32; 4] {
        let visible = self.visible();
        #[allow(clippy::cast_possible_truncation)]
        [
            visible.x0 as f32,
            visible.y0 as f32,
            visible.width().max(1e-3) as f32,
            visible.height().max(1e-3) as f32,
        ]
    }

    /// The scene's coordinates onto the view's. Tracked.
    #[must_use]
    pub fn to_screen(&self) -> Affine {
        let (x, y) = self.scroll.get();
        let zoom = self.zoom.get();
        Affine::scale(zoom) * Affine::translate((-x, -y))
    }

    /// The view's coordinates onto the scene's. Tracked.
    #[must_use]
    pub fn to_scene(&self) -> Affine {
        self.to_screen().inverse()
    }

    /// `at` in the scene, as a point in the view. Tracked.
    ///
    /// The tracked twin of [`Viewport::screen_point`], for anything that has to be re-placed when
    /// the view moves — words and pictures, which are laid out rather than painted.
    #[must_use]
    pub fn place(&self, at: Point) -> Point {
        let (x, y) = self.scroll.get();
        let zoom = self.zoom.get();
        Point::new((at.x - x) * zoom, (at.y - y) * zoom)
    }

    /// `at` in the view, as a point in the scene. Not tracked, for a pointer handler.
    #[must_use]
    pub fn scene_point(&self, at: Point) -> Point {
        let (x, y) = self.scroll.get_untracked();
        let zoom = self.zoom.get_untracked().max(f64::EPSILON);
        Point::new(x + at.x / zoom, y + at.y / zoom)
    }

    /// `at` in the scene, as a point in the view. Not tracked.
    #[must_use]
    pub fn screen_point(&self, at: Point) -> Point {
        let (x, y) = self.scroll.get_untracked();
        let zoom = self.zoom.get_untracked();
        Point::new((at.x - x) * zoom, (at.y - y) * zoom)
    }

    /// Moves the view by `by` view pixels.
    pub fn pan_by(&self, by: Vec2) {
        let zoom = self.zoom.get_untracked().max(f64::EPSILON);
        let (x, y) = self.scroll.get_untracked();
        self.scroll.set((x - by.x / zoom, y - by.y / zoom));
    }

    /// Moves it to put `at` at the view's top left corner.
    pub fn scroll_to(&self, at: Point) {
        self.scroll.set((at.x, at.y));
    }

    /// Multiplies the scale, keeping the scene under `focus` where it is.
    ///
    /// `focus` is in view pixels. Without one, the middle of the view holds still.
    pub fn zoom_by(&self, factor: f64, focus: Option<Point>) {
        let held = self.zoom.get_untracked();
        let next = (held * factor).clamp(NEAREST, FARTHEST);
        if (next - held).abs() < f64::EPSILON {
            return;
        }
        let (width, height) = self.size.get_untracked();
        let focus = focus.unwrap_or_else(|| Point::new(width / 2.0, height / 2.0));
        let under = self.scene_point(focus);
        self.zoom.set(next);
        // The same scene point, put back under the same view point.
        self.scroll
            .set((under.x - focus.x / next, under.y - focus.y / next));
    }

    /// Sets the scale outright, keeping the middle of the view where it is.
    pub fn zoom_to(&self, scale: f64) {
        let held = self.zoom.get_untracked();
        if held > 0.0 {
            self.zoom_by(scale.clamp(NEAREST, FARTHEST) / held, None);
        }
    }

    /// One scene unit to one view pixel, keeping the middle where it is.
    pub fn actual(&self) {
        self.zoom_to(1.0);
    }

    /// Puts `bounds` on screen with a little room around it.
    ///
    /// Nothing to fit leaves the view where it is at one to one, which is what an empty drawing
    /// wants.
    pub fn fit(&self, bounds: Option<Rect>) {
        let (width, height) = self.size.get_untracked();
        let Some(bounds) = bounds else {
            self.zoom.set(1.0);
            self.scroll.set((-width / 2.0, -height / 2.0));
            return;
        };
        if width <= 0.0 || height <= 0.0 || bounds.width() <= 0.0 || bounds.height() <= 0.0 {
            return;
        }
        let margin = 0.9;
        let zoom = ((width / bounds.width()).min(height / bounds.height()) * margin)
            .clamp(NEAREST, FARTHEST);
        self.zoom.set(zoom);
        let center = bounds.center();
        self.scroll.set((
            center.x - width / (2.0 * zoom),
            center.y - height / (2.0 * zoom),
        ));
    }

    /// The factor one wheel notch of `delta` view pixels changes the scale by.
    #[must_use]
    pub fn wheel_factor(delta: f64) -> f64 {
        WHEEL_STEP.powf(-delta / LINE)
    }
}

impl Default for Viewport {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measured() -> Viewport {
        let viewport = Viewport::new();
        viewport.set_size(400.0, 300.0);
        viewport
    }

    #[test]
    fn a_point_round_trips_through_both_spaces() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let viewport = measured();
            viewport.zoom_to(2.5);
            viewport.pan_by(Vec2::new(-30.0, 17.0));
            let at = Point::new(123.0, 45.0);
            let back = viewport.screen_point(viewport.scene_point(at));
            assert!((back - at).hypot() < 1e-9);
        });
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer_still() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let viewport = measured();
            let focus = Point::new(300.0, 100.0);
            let before = viewport.scene_point(focus);
            viewport.zoom_by(2.0, Some(focus));
            let after = viewport.scene_point(focus);
            assert!((before - after).hypot() < 1e-9);
        });
    }

    #[test]
    fn the_zoom_stays_between_its_ends() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let viewport = measured();
            viewport.zoom_to(1000.0);
            assert!((viewport.zoom_untracked() - FARTHEST).abs() < f64::EPSILON);
            viewport.zoom_to(0.0001);
            assert!((viewport.zoom_untracked() - NEAREST).abs() < f64::EPSILON);
        });
    }

    #[test]
    fn a_pan_moves_the_scene_the_way_the_pointer_went() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let viewport = measured();
            let before = viewport.scene_point(Point::ZERO);
            viewport.pan_by(Vec2::new(50.0, 0.0));
            let after = viewport.scene_point(Point::ZERO);
            assert!(after.x < before.x, "the scene came with the pointer");
        });
    }

    #[test]
    fn placing_a_point_follows_the_view() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let viewport = measured();
            let at = Point::new(100.0, 50.0);
            let before = viewport.place(at);
            viewport.pan_by(Vec2::new(-40.0, 0.0));
            let after = viewport.place(at);
            assert!(
                (after.x - (before.x - 40.0)).abs() < 1e-9,
                "it moved with the view: {before:?} to {after:?}"
            );
            // And it agrees with the untracked twin the pointer uses.
            assert!((after - viewport.screen_point(at)).hypot() < 1e-9);
        });
    }

    #[test]
    fn the_view_box_is_exactly_what_is_visible() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let viewport = measured();
            viewport.zoom_to(2.0);
            let [x, y, width, height] = viewport.view_box();
            let visible = viewport.visible();
            assert!((f64::from(x) - visible.x0).abs() < 1e-3);
            assert!((f64::from(y) - visible.y0).abs() < 1e-3);
            assert!((f64::from(width) - 200.0).abs() < 1e-3);
            assert!((f64::from(height) - 150.0).abs() < 1e-3);
        });
    }

    #[test]
    fn fitting_puts_the_drawing_in_the_middle() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let viewport = measured();
            let bounds = Rect::new(1000.0, 1000.0, 1200.0, 1100.0);
            viewport.fit(Some(bounds));
            let middle = viewport.scene_point(Point::new(200.0, 150.0));
            assert!((middle - bounds.center()).hypot() < 1e-6);
            assert!(viewport.visible().contains(bounds.center()));
        });
    }

    #[test]
    fn fitting_nothing_looks_at_the_origin() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let viewport = measured();
            viewport.fit(None);
            let middle = viewport.scene_point(Point::new(200.0, 150.0));
            assert!(middle.to_vec2().hypot() < 1e-9);
        });
    }
}
