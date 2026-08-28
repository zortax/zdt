//! A zoomable, pannable plane for a rich view's content.
//!
//! The image and SVG previews both show one thing at a chosen scale. The camera holds that
//! scale and the pan, the [`Stage`] applies them, and the keys reach the camera through a
//! [`Stages`] registry the way the scroll keys reach a [`super::Reading`].
//!
//! The plane is sized in layout, and a zoom is a new explicit size. A transform would move the
//! paint alone, and an image element decodes for the size its box is laid out at.

use std::cell::RefCell;
use std::rc::Rc;

use rustc_hash::FxHashMap;
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RenderEffect, RwSignal};
use zgui::{component, view};

use crate::workspace::{BufferId, WindowId, Workspace};

/// The smallest and largest scale a zoom can reach.
const NEAREST: f32 = 0.05;
const FARTHEST: f32 = 32.0;

/// How much one zoom step changes the scale.
pub const STEP: f32 = 1.25;

/// How much one wheel notch changes the scale.
const WHEEL_STEP: f32 = 1.2;

/// How far one pan key moves the content, in CSS pixels.
const NUDGE: f32 = 48.0;

/// One line of wheel travel, in CSS pixels.
const LINE: f32 = 16.0;

/// One view's place over its content: how large the content is drawn, and where it sits.
///
/// All lengths are CSS pixels. The pan is the plane's offset from its centred position, so a
/// fresh camera shows the content centred at the fitted scale.
#[derive(Clone, Copy, PartialEq)]
pub struct Camera {
    /// The content's natural size, once it is known.
    content: RwSignal<Option<(f32, f32)>, LocalStorage>,
    /// The chosen scale. Fit when none is chosen.
    zoom: RwSignal<Option<f32>, LocalStorage>,
    /// How far the plane is from centred.
    pan: RwSignal<(f32, f32), LocalStorage>,
    /// How large the stage is, from the measured view.
    viewport: RwSignal<(f32, f32), LocalStorage>,
}

impl Camera {
    /// Fitted and centred, before anything is measured.
    #[must_use]
    pub fn new() -> Self {
        Self {
            content: RwSignal::new_local(None),
            zoom: RwSignal::new_local(None),
            pan: RwSignal::new_local((0.0, 0.0)),
            viewport: RwSignal::new_local((0.0, 0.0)),
        }
    }

    /// Says how large the content is.
    pub fn set_content(&self, width: f32, height: f32) {
        if width > 0.0 && height > 0.0 {
            self.content.set(Some((width, height)));
        }
    }

    /// The content's natural size, when it is known. Tracked.
    #[must_use]
    pub fn content(&self) -> Option<(f32, f32)> {
        self.content.get()
    }

    /// Says how large the stage is, from the measured view.
    pub fn set_viewport(&self, width: f32, height: f32) {
        if self.viewport.get_untracked() != (width, height) {
            self.viewport.set((width, height));
            self.pan.set(self.clamped(self.pan.get_untracked()));
        }
    }

    /// The scale the content is drawn at. Tracked.
    ///
    /// Fit is contain, capped at one: a small picture is shown at its own size, and a large one
    /// is shrunk until all of it is on screen.
    #[must_use]
    pub fn scale(&self) -> f32 {
        match self.zoom.get() {
            Some(zoom) => zoom,
            None => self.fitted(),
        }
    }

    /// The fitted scale, from the measured sizes.
    fn fitted(&self) -> f32 {
        let Some((width, height)) = self.content.get() else {
            return 1.0;
        };
        let (view_width, view_height) = self.viewport.get();
        if view_width <= 0.0 || view_height <= 0.0 {
            return 1.0;
        }
        (view_width / width).min(view_height / height).min(1.0)
    }

    /// Where the plane sits: left, top, width and height in CSS pixels. Tracked.
    #[must_use]
    pub fn placement(&self) -> Option<(f32, f32, f32, f32)> {
        let (width, height) = self.content.get()?;
        let scale = self.scale();
        let (plane_width, plane_height) = (width * scale, height * scale);
        let (view_width, view_height) = self.viewport.get();
        let (pan_x, pan_y) = self.pan.get();
        Some((
            (view_width - plane_width) / 2.0 + pan_x,
            (view_height - plane_height) / 2.0 + pan_y,
            plane_width,
            plane_height,
        ))
    }

    /// Multiplies the scale, keeping the content under `focus` where it is.
    ///
    /// `focus` is in CSS pixels from the stage's top-left corner. Without one, the centre of the
    /// stage holds still.
    pub fn zoom_by(&self, factor: f32, focus: Option<(f32, f32)>) {
        let Some((width, height)) = self.content.get_untracked() else {
            return;
        };
        let (view_width, view_height) = self.viewport.get_untracked();
        let held = match self.zoom.get_untracked() {
            Some(zoom) => zoom,
            None => self.fitted_untracked(),
        };
        let next = (held * factor).clamp(NEAREST, FARTHEST);
        let (focus_x, focus_y) = focus.unwrap_or((view_width / 2.0, view_height / 2.0));

        // The content point under the focus, kept there through the change of scale.
        let (pan_x, pan_y) = self.pan.get_untracked();
        let left = (view_width - width * held) / 2.0 + pan_x;
        let top = (view_height - height * held) / 2.0 + pan_y;
        let (at_x, at_y) = ((focus_x - left) / held, (focus_y - top) / held);
        let left = focus_x - at_x * next;
        let top = focus_y - at_y * next;
        let pan = (
            left - (view_width - width * next) / 2.0,
            top - (view_height - height * next) / 2.0,
        );

        self.zoom.set(Some(next));
        self.pan.set(self.clamped_at(pan, next));
    }

    /// Sets the scale outright, keeping the centre where it is.
    pub fn zoom_to(&self, scale: f32) {
        let held = match self.zoom.get_untracked() {
            Some(zoom) => zoom,
            None => self.fitted_untracked(),
        };
        if held > 0.0 {
            self.zoom_by(scale.clamp(NEAREST, FARTHEST) / held, None);
        }
    }

    /// Back to fitted and centred.
    pub fn fit(&self) {
        self.zoom.set(None);
        self.pan.set((0.0, 0.0));
    }

    /// One content pixel to one CSS pixel, centred.
    pub fn actual(&self) {
        self.zoom.set(Some(1.0));
        self.pan.set(self.clamped_at((0.0, 0.0), 1.0));
    }

    /// Between fitted and one-to-one, which a double click asks for.
    pub fn toggle_fit(&self) {
        if self.zoom.get_untracked().is_some() {
            self.fit();
        } else {
            self.actual();
        }
    }

    /// Moves the content by `(dx, dy)` CSS pixels.
    pub fn pan_by(&self, dx: f32, dy: f32) {
        let (pan_x, pan_y) = self.pan.get_untracked();
        self.pan.set(self.clamped((pan_x + dx, pan_y + dy)));
    }

    /// `pan`, kept where there is content to see, at the held scale.
    fn clamped(&self, pan: (f32, f32)) -> (f32, f32) {
        let scale = match self.zoom.get_untracked() {
            Some(zoom) => zoom,
            None => self.fitted_untracked(),
        };
        self.clamped_at(pan, scale)
    }

    /// The same, at `scale`.
    ///
    /// An axis on which the content fits stays centred. On the other, the plane's edges stay at
    /// or past the stage's, so no blank margin opens up inside a zoomed view.
    fn clamped_at(&self, pan: (f32, f32), scale: f32) -> (f32, f32) {
        let Some((width, height)) = self.content.get_untracked() else {
            return (0.0, 0.0);
        };
        let (view_width, view_height) = self.viewport.get_untracked();
        let clamp = |pan: f32, content: f32, view: f32| {
            let spare = (content * scale - view) / 2.0;
            if spare <= 0.0 {
                0.0
            } else {
                pan.clamp(-spare, spare)
            }
        };
        (
            clamp(pan.0, width, view_width),
            clamp(pan.1, height, view_height),
        )
    }

    /// The fitted scale, without subscribing.
    fn fitted_untracked(&self) -> f32 {
        let Some((width, height)) = self.content.get_untracked() else {
            return 1.0;
        };
        let (view_width, view_height) = self.viewport.get_untracked();
        if view_width <= 0.0 || view_height <= 0.0 {
            return 1.0;
        }
        (view_width / width).min(view_height / height).min(1.0)
    }
}

impl Default for Camera {
    fn default() -> Self {
        Self::new()
    }
}

/// The device-pixel density at `node`, made safe to divide by.
pub(crate) fn density_of(node: NodeRef) -> f32 {
    super::density(node.scale())
}

/// Every mounted stage's camera, by the window and buffer it belongs to.
///
/// No signal, the same reasoning as [`super::Previews`]: nothing on screen is decided by which
/// stages exist, and a key that zooms one needs it right now.
#[derive(Clone)]
pub struct Stages {
    inner: Rc<RefCell<FxHashMap<(WindowId, BufferId), Camera>>>,
}

impl Stages {
    /// Nothing mounted.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(FxHashMap::default())),
        }
    }

    /// Remembers the camera of the stage showing `buffer` in `window`.
    pub fn register(&self, window: WindowId, buffer: BufferId, camera: Camera) {
        self.inner.borrow_mut().insert((window, buffer), camera);
    }

    /// Forgets it, which a view does as it unmounts.
    ///
    /// Dropped only when `camera` is still the one filed here, so a view rebuilt in place that
    /// registers its replacement first keeps the replacement.
    pub fn forget(&self, window: WindowId, buffer: BufferId, camera: Camera) {
        let mut held = self.inner.borrow_mut();
        if held.get(&(window, buffer)) == Some(&camera) {
            held.remove(&(window, buffer));
        }
    }

    /// The camera of the stage the keyboard is in, when it is in one.
    #[must_use]
    pub fn current(&self, workspace: &Workspace) -> Option<Camera> {
        let window = workspace.focused_untracked();
        let buffer = workspace.buffer_in_untracked(window)?;
        if !workspace.is_rich_untracked(window, buffer) {
            return None;
        }
        self.inner.borrow().get(&(window, buffer)).copied()
    }
}

impl Default for Stages {
    fn default() -> Self {
        Self::new()
    }
}

/// Puts the registry where every component can find it.
pub fn provide(stages: Stages) {
    zgui::reactive::provide_local_context(stages);
}

/// It, from inside a component.
///
/// # Panics
///
/// If none was provided above this component, which is a wiring mistake.
#[must_use]
pub fn use_stages() -> Stages {
    zgui::reactive::use_local_context::<Stages>().expect("stages are provided at the root")
}

/// The stage: a clipped viewport whose one child plane the camera places.
///
/// The wheel zooms around the pointer, a drag pans, and a double click flips between fitted and
/// one-to-one. What the plane holds is the caller's. The plane glides to each placement through
/// the sheet's transition, and wears `data-dragging` while a drag pans so the glide stands down
/// and the content tracks the pointer exactly.
#[component]
pub fn Stage(
    /// The camera that places the plane.
    camera: Camera,
    /// What the plane shows.
    children: Children,
) -> impl IntoView {
    let node = NodeRef::new();

    // The measured stage, kept on the camera so fit and clamping follow every resize.
    {
        let size = node.observe_content_size();
        let measuring = RenderEffect::new(move |_| {
            let measured = size.get();
            let scale = super::density(node.scale());
            camera.set_viewport(measured.width.0 / scale, measured.height.0 / scale);
        });
        on_cleanup_local(move || drop(measuring));
    }

    // Where a drag started: the pointer, and the pan it found.
    let from: RwSignal<Option<(f32, f32, f32, f32)>, LocalStorage> = RwSignal::new_local(None);

    // The pointer in CSS pixels from the stage's top-left corner.
    //
    // The stage's own measured box, asked of the node. The event answers positions from the
    // window's corner, and the difference between the two corners is everything left and above
    // the stage: the file tree, the tab line.
    let local =
        move |position: zgui::geom::Point<zgui::geom::CssPx, zgui::geom::Css>| -> (f32, f32) {
            let scale = super::density(node.scale());
            let (left, top) = node
                .window_bounds()
                .map(|held| (held.origin.x.0 / scale, held.origin.y.0 / scale))
                .unwrap_or((0.0, 0.0));
            (position.x.0 - left, position.y.0 - top)
        };

    let place = move |take: fn((f32, f32, f32, f32)) -> f32| {
        move || camera.placement().map(|held| format!("{}px", take(held)))
    };

    view! {
        box(
            class = "stage",
            node_ref = node,
            attr:data-dragging = move || from.get().map(|_| "true".to_owned()),
            on:wheel = move |ev: &mut EventCx<'_, events::Wheel>| {
                let delta = ev.delta.to_pixels(zgui::geom::CssPx(LINE));
                let factor = WHEEL_STEP.powf(-delta.height.0 / LINE);
                let focus = local(ev.position);
                camera.zoom_by(factor, Some(focus));
                ev.prevent_default();
                ev.stop_propagation();
            },
            on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                if !ev.primary {
                    return;
                }
                let (pan_x, pan_y) = camera.pan.get_untracked();
                let (x, y) = (ev.position.x.0, ev.position.y.0);
                from.set(Some((x, y, pan_x, pan_y)));
                ev.capture_pointer();
            },
            on:pointer_move = move |ev: &mut EventCx<'_, events::PointerMove>| {
                if let Some((x, y, pan_x, pan_y)) = from.get_untracked() {
                    let moved = (
                        pan_x + (ev.position.x.0 - x),
                        pan_y + (ev.position.y.0 - y),
                    );
                    camera.pan.set(camera.clamped(moved));
                }
            },
            on:pointer_up = move |ev: &mut EventCx<'_, events::PointerUp>| {
                from.set(None);
                ev.release_pointer();
            },
            on:pointer_cancel = move |ev: &mut EventCx<'_, events::PointerCancel>| {
                from.set(None);
                ev.release_pointer();
            },
            on:double_click = move |_: &mut EventCx<'_, events::DoubleClick>| {
                camera.toggle_fit();
            }
        ) {
            box(
                class = "stage__plane",
                style:left = place(|held| held.0),
                style:top = place(|held| held.1),
                style:width = place(|held| held.2),
                style:height = place(|held| held.3)
            ) {
                {children.into_view_once()}
            }
        }
    }
}

/// Zooming and panning the stage under the keyboard, from the keys of a preview's region.
pub fn run(workspace: &Workspace, leaf: &str) {
    let Some(stages) = zgui::reactive::use_local_context::<Stages>() else {
        return;
    };
    let Some(camera) = stages.current(workspace) else {
        return;
    };

    match leaf {
        "zoom_in" => camera.zoom_by(STEP, None),
        "zoom_out" => camera.zoom_by(1.0 / STEP, None),
        "fit" => camera.fit(),
        "actual" => camera.actual(),
        "pan_left" => camera.pan_by(NUDGE, 0.0),
        "pan_right" => camera.pan_by(-NUDGE, 0.0),
        "pan_up" => camera.pan_by(0.0, NUDGE),
        "pan_down" => camera.pan_by(0.0, -NUDGE),
        // Silently. The base map layers underneath the region, and an unbound key there falls
        // through to it.
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measured() -> Camera {
        let camera = Camera::new();
        camera.set_content(400.0, 200.0);
        camera.set_viewport(200.0, 200.0);
        camera
    }

    #[test]
    fn fit_contains_and_never_enlarges() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let camera = measured();
            assert!((camera.scale() - 0.5).abs() < f32::EPSILON);

            // Content smaller than the stage is shown at its own size.
            camera.set_content(50.0, 50.0);
            assert!((camera.scale() - 1.0).abs() < f32::EPSILON);
        });
    }

    #[test]
    fn zooming_keeps_the_focused_point_still() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let camera = measured();
            let focus = (150.0, 100.0);

            let at = |camera: &Camera, focus: (f32, f32)| {
                let (left, top, width, _) = camera.placement().expect("measured");
                let scale = width / 400.0;
                ((focus.0 - left) / scale, (focus.1 - top) / scale)
            };

            let before = at(&camera, focus);
            camera.zoom_by(2.0, Some(focus));
            let after = at(&camera, focus);
            assert!((before.0 - after.0).abs() < 0.01);
            assert!((before.1 - after.1).abs() < 0.01);
        });
    }

    #[test]
    fn a_pan_cannot_open_a_margin_inside_a_zoomed_view() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let camera = measured();
            camera.zoom_to(2.0);
            camera.pan_by(10_000.0, 10_000.0);
            let (left, top, width, height) = camera.placement().expect("measured");
            assert!(left <= 0.0);
            assert!(top <= 0.0);
            assert!(left + width >= 200.0);
            assert!(top + height >= 200.0);
        });
    }

    #[test]
    fn the_axis_that_fits_stays_centred() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let camera = measured();
            camera.pan_by(50.0, 50.0);
            let (left, top, ..) = camera.placement().expect("measured");
            // Fitted at 0.5: the plane is 200 by 100, so both axes fit and both stay centred.
            assert!(left.abs() < f32::EPSILON);
            assert!((top - 50.0).abs() < f32::EPSILON);
        });
    }
}
