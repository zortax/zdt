//! What one mounted editor holds.
//!
//! Every field is a signal handle, so the whole of it copies: a key handler, a pointer handler and
//! a draw closure each hold one by value and all of them see the same drawing.

use std::rc::Rc;

use excalidraw::{Command, Id, Scene};
use kurbo::{Point, Rect, Vec2};
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal, StoredValue};

use crate::viewport::Viewport;

/// How solid something the eraser has marked is drawn.
pub const MARKED_TO_ERASE: f64 = 0.2;

/// Which surface a drawing is shown on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Scheme {
    /// A light one.
    Light,
    /// A dark one.
    Dark,
    /// Whichever the desktop asked for.
    #[default]
    System,
}

/// Which tool the pointer is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tool {
    /// Take hold of what is there.
    #[default]
    Select,
    /// Move the view.
    Hand,
    /// Draw a rectangle.
    Rectangle,
    /// Draw a diamond.
    Diamond,
    /// Draw an ellipse.
    Ellipse,
    /// Draw an arrow.
    Arrow,
    /// Draw a line.
    Line,
    /// Draw by hand.
    Freedraw,
    /// Write.
    Text,
    /// Place a picture.
    Image,
    /// Draw a frame.
    Frame,
    /// Rub out.
    Eraser,
}

impl Tool {
    /// The kind of element this tool draws by dragging, when it draws one.
    ///
    /// Words are not among them: they are placed by one press, because there is no box to drag.
    #[must_use]
    pub const fn kind(self) -> Option<excalidraw::Kind> {
        use excalidraw::Kind as K;
        Some(match self {
            Self::Rectangle => K::Rectangle,
            Self::Diamond => K::Diamond,
            Self::Ellipse => K::Ellipse,
            Self::Arrow => K::Arrow,
            Self::Line => K::Line,
            Self::Freedraw => K::Freedraw,
            Self::Image => K::Image,
            Self::Frame => K::Frame,
            _ => return None,
        })
    }

    /// Whether it draws by dragging a box open.
    #[must_use]
    pub const fn drags_a_box(self) -> bool {
        matches!(
            self,
            Self::Rectangle | Self::Diamond | Self::Ellipse | Self::Frame | Self::Image
        )
    }

    /// Whether it draws by walking a run of points.
    #[must_use]
    pub const fn walks_points(self) -> bool {
        matches!(self, Self::Arrow | Self::Line)
    }

    /// How wide a new element of this tool's kind is drawn.
    ///
    /// A pen stroke is stored at half the width of everything else: the outline it is filled with
    /// is over four times its stored width, so the two read the same on the page.
    #[must_use]
    pub fn stroke_width(self, chosen: f64) -> f64 {
        if matches!(self, Self::Freedraw) {
            chosen / 2.0
        } else {
            chosen
        }
    }
}

/// Which handle a resize is dragging.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sides {
    /// Whether the left edge moves.
    pub left: bool,
    /// The right.
    pub right: bool,
    /// The top.
    pub top: bool,
    /// The bottom.
    pub bottom: bool,
}

/// What one drag is doing.
#[derive(Clone, PartialEq, Debug)]
pub enum Drag {
    /// Moving the view.
    ///
    /// Both of these are taken when the press lands and never again. A pan measured against the
    /// scroll it is itself changing would chase its own tail: the pointer's place in the drawing
    /// moves as the drawing moves under it, so the movement would be counted against a frame that
    /// had already shifted.
    Pan {
        /// Where the view was looking when it started.
        from: (f64, f64),
        /// Where the pointer was, in the view's own pixels.
        at: Point,
    },
    /// Choosing what to select with a band.
    Band,
    /// Moving the selection.
    Move,
    /// Scaling it by a handle.
    ///
    /// Everything here is in the drawing's own units, so what the drag is doing can be worked out
    /// without asking where the view is looking — which is what keeps a pan from re-drawing every
    /// shape on the page.
    Resize {
        /// Which edges the handle moves.
        sides: Sides,
        /// The box the selection was in when the drag started, upright.
        from: Rect,
        /// How far that box is turned.
        angle: f64,
        /// What it turns about.
        about: Point,
    },
    /// Moving one point of a line or an arrow.
    ///
    /// The points are carried whole, already holding whatever the press put in — a press on the
    /// middle of a segment adds a point there before the drag begins, so the drag is the same
    /// whether the point was already there or has just appeared.
    Point {
        /// Which line.
        id: Id,
        /// Its points, in the scene.
        points: Vec<Point>,
        /// Which one is being moved.
        at: usize,
    },
    /// Turning it.
    Rotate {
        /// What it turns about.
        about: Point,
        /// Which way the pointer pointed when the drag started.
        start: f64,
    },
    /// Drawing a new element by dragging its box open.
    DrawBox {
        /// Which kind.
        kind: excalidraw::Kind,
    },
    /// Drawing one by hand.
    DrawFree {
        /// Where the pen has been, in the scene.
        points: Vec<Point>,
        /// How hard it was pressed at each point.
        pressures: Vec<f64>,
    },
    /// Walking a run of points for a line or an arrow.
    DrawPoints {
        /// Which kind.
        kind: excalidraw::Kind,
        /// The points so far, in the scene.
        points: Vec<Point>,
    },
    /// Rubbing out whatever the pointer passes over.
    Erase {
        /// What has been rubbed out so far.
        hit: Vec<Id>,
        /// A generous box around each thing that could be rubbed out.
        ///
        /// Measured once, when the drag starts. Asking what is under the pointer means drawing
        /// every shape in the drawing to see where it goes, and the pointer asks sixty times a
        /// second — so the question is only put when a box says the answer might be yes.
        reach: Rc<Vec<(Id, Rect)>>,
    },
}

/// One live drag.
#[derive(Clone, PartialEq, Debug)]
pub struct Live {
    /// What it is doing.
    pub drag: Drag,
    /// Where it started, in the scene.
    pub from: Point,
    /// Where the pointer is now, in the scene.
    pub at: Point,
    /// Whether the shift key is down, which constrains it.
    pub constrained: bool,
    /// Whether the alt key is, which works about the middle.
    pub from_center: bool,
}

impl Live {
    /// How far the pointer has come.
    #[must_use]
    pub fn delta(&self) -> Vec2 {
        self.at - self.from
    }

    /// The box the pointer has dragged open, with the shift and alt keys taken into account.
    #[must_use]
    pub fn box_(&self) -> Rect {
        let delta = self.delta();
        let (mut width, mut height) = (delta.x, delta.y);
        if self.constrained {
            // A square, in whichever direction the pointer went.
            let side = width.abs().max(height.abs());
            width = side * width.signum();
            height = side * height.signum();
        }
        if self.from_center {
            return Rect::from_center_size(self.from, (width.abs() * 2.0, height.abs() * 2.0));
        }
        Rect::from_points(
            self.from,
            Point::new(self.from.x + width, self.from.y + height),
        )
    }
}

/// One mounted editor.
#[derive(Clone, Copy)]
pub struct Board {
    /// The drawing, and what is selected in it.
    pub scene: RwSignal<Rc<Scene>, LocalStorage>,
    /// Where it is looked at from.
    pub viewport: Viewport,
    /// Which tool the pointer is.
    pub tool: RwSignal<Tool, LocalStorage>,
    /// The drag under way, when there is one.
    pub live: RwSignal<Option<Live>, LocalStorage>,
    /// What the eraser has passed over, and will take away when the pointer comes up.
    ///
    /// Its own signal rather than a look inside the drag: it changes only when something new is
    /// marked, so a band that shows the marks is not painted again on every movement.
    pub erasing: RwSignal<Rc<rustc_hash::FxHashSet<Id>>, LocalStorage>,
    /// Where the pointer is, in the view's own pixels, while a tool wants to draw its own.
    pub pointer: RwSignal<Option<Point>, LocalStorage>,
    /// The line whose points are being moved, which the chrome draws in place of the drawing's.
    ///
    /// Its own signal, changing only when such a drag starts and ends, so a band is not painted
    /// again for every movement of the point.
    pub editing_points: RwSignal<Option<Id>, LocalStorage>,
    /// Whether that drag is moving what is already drawn.
    ///
    /// Held apart from the drag itself so that asking is cheap and, more to the point, so that a
    /// band which holds nothing being moved never reads the drag — and is therefore not painted
    /// again on every movement of the pointer.
    pub moving: RwSignal<bool, LocalStorage>,
    /// The words being typed, when any are.
    pub editing: RwSignal<Option<Id>, LocalStorage>,
    /// What has been typed into them so far.
    pub typing: RwSignal<String, LocalStorage>,
    /// Which surface the host says the drawing is shown on.
    pub scheme: RwSignal<Scheme, LocalStorage>,
    /// Whether the desktop asked for a dark one, as the style engine answered it.
    pub prefers_dark: RwSignal<bool, LocalStorage>,
    /// Whether the properties panel is out.
    pub panel: RwSignal<bool, LocalStorage>,
    /// What the corner has to say.
    pub notice: RwSignal<Option<String>, LocalStorage>,
    /// How many changes have been made here, which the host watches to know when to write.
    pub revision: RwSignal<u64, LocalStorage>,
    /// The shapes each element was last drawn as.
    ///
    /// Held rather than made again: a band is painted again whenever anything it shows changes,
    /// and drawing a pen stroke by hand costs the same whether the stroke changed or not.
    pub drawn: StoredValue<excalidraw::draw::Cache, LocalStorage>,
}

impl Board {
    /// An editor over `scene`.
    #[must_use]
    pub fn new(scene: Scene) -> Self {
        Self {
            scene: RwSignal::new_local(Rc::new(scene)),
            viewport: Viewport::new(),
            tool: RwSignal::new_local(Tool::Select),
            live: RwSignal::new_local(None),
            moving: RwSignal::new_local(false),
            erasing: RwSignal::new_local(Rc::new(rustc_hash::FxHashSet::default())),
            pointer: RwSignal::new_local(None),
            editing_points: RwSignal::new_local(None),
            editing: RwSignal::new_local(None),
            typing: RwSignal::new_local(String::new()),
            scheme: RwSignal::new_local(Scheme::System),
            prefers_dark: RwSignal::new_local(false),
            panel: RwSignal::new_local(false),
            notice: RwSignal::new_local(None),
            revision: RwSignal::new_local(0),
            drawn: StoredValue::new_local(excalidraw::draw::Cache::new()),
        }
    }

    /// Does `command`, and answers whether anything changed.
    ///
    /// A command that changes nothing moves no revision, so a drag that ended where it began costs
    /// the host nothing.
    pub fn apply(&self, command: Command) -> bool {
        let mut moved = false;
        self.scene.update(|held| {
            moved = Rc::make_mut(held).apply(command);
        });
        if moved {
            self.revision.update(|held| *held += 1);
            // What is gone is forgotten, so a drawing edited all afternoon holds no more shapes
            // than it shows.
            let scene = self.scene.get_untracked();
            self.drawn
                .update_value(|cache| cache.retain(scene.elements()));
        }
        moved
    }

    /// Changes the scene without going through a command.
    ///
    /// For what is not an edit — the selection, the style a new element takes — so the revision
    /// stays a count of what the file would be written for.
    pub fn with_scene(&self, change: impl FnOnce(&mut Scene)) {
        self.scene.update(|held| change(Rc::make_mut(held)));
    }

    /// The scene, as it is now. Tracked.
    #[must_use]
    pub fn read(&self) -> Rc<Scene> {
        self.scene.get()
    }

    /// The same, without subscribing.
    #[must_use]
    pub fn read_untracked(&self) -> Rc<Scene> {
        self.scene.get_untracked()
    }

    /// Whether the drawing is painted for a dark surface. Tracked.
    ///
    /// A drawing keeps the colours it was drawn with either way; this only decides how they are
    /// painted, the same as Excalidraw's own dark mode.
    #[must_use]
    pub fn dark(&self) -> bool {
        match self.scheme.get() {
            Scheme::Light => false,
            Scheme::Dark => true,
            Scheme::System => self.prefers_dark.get(),
        }
    }

    /// How solid `id` is drawn, against what the element itself says.
    ///
    /// What the eraser has marked is faded rather than taken away, so the reader can see what is
    /// about to go and let go of the pointer if it is the wrong thing.
    #[must_use]
    pub fn fade(&self, id: &Id) -> f64 {
        // A line whose points are being moved is drawn by the chrome instead, from the points the
        // drag is holding. Drawing it here as well would leave the old shape under the new one.
        if self.editing_points.get().as_ref() == Some(id) {
            return 0.0;
        }
        if self.erasing.get().contains(id) {
            MARKED_TO_ERASE
        } else {
            1.0
        }
    }

    /// The style a new element takes, with what the tool asks of it.
    #[must_use]
    pub fn style_for(&self, tool: Tool) -> excalidraw::scene::Style {
        let mut style = self.read_untracked().style.clone();
        style.stroke_width = tool.stroke_width(style.stroke_width);
        style
    }
}

impl PartialEq for Board {
    fn eq(&self, other: &Self) -> bool {
        self.scene == other.scene && self.viewport == other.viewport
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene() -> Scene {
        let drawing = excalidraw::file::parse(
            r#"{"type":"excalidraw","elements":[{"type":"rectangle","id":"a","x":0,"y":0}]}"#,
        )
        .expect("a drawing");
        Scene::new(drawing, 1, 1)
    }

    #[test]
    fn a_command_that_changes_something_moves_the_revision() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let board = Board::new(scene());
            assert!(board.apply(Command::Translate {
                ids: vec![Id::new("a")],
                by: Vec2::new(5.0, 0.0),
            }));
            assert_eq!(board.revision.get_untracked(), 1);
        });
    }

    #[test]
    fn a_command_that_changes_nothing_does_not() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let board = Board::new(scene());
            assert!(!board.apply(Command::Translate {
                ids: vec![Id::new("a")],
                by: Vec2::ZERO,
            }));
            assert_eq!(board.revision.get_untracked(), 0);
        });
    }

    #[test]
    fn the_scheme_the_host_names_wins_over_the_desktops() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let board = Board::new(scene());
            board.prefers_dark.set(true);
            assert!(board.dark(), "the desktop decides when nothing else does");

            board.scheme.set(Scheme::Light);
            assert!(!board.dark());
            board.scheme.set(Scheme::Dark);
            board.prefers_dark.set(false);
            assert!(board.dark());
        });
    }

    #[test]
    fn a_pen_stroke_is_stored_at_half_the_width_of_everything_else() {
        assert!((Tool::Freedraw.stroke_width(2.0) - 1.0).abs() < f64::EPSILON);
        assert!((Tool::Rectangle.stroke_width(2.0) - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_tool_stays_chosen_until_another_one_is() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let board = Board::new(scene());
            board.tool.set(Tool::Rectangle);
            assert_eq!(board.tool.get_untracked(), Tool::Rectangle);
            board.tool.set(Tool::Freedraw);
            assert_eq!(board.tool.get_untracked(), Tool::Freedraw);
        });
    }

    #[test]
    fn shift_drags_a_square_and_alt_drags_from_the_middle() {
        let live = |constrained, from_center| Live {
            drag: Drag::DrawBox {
                kind: excalidraw::Kind::Rectangle,
            },
            from: Point::new(10.0, 10.0),
            at: Point::new(110.0, 50.0),
            constrained,
            from_center,
        };
        let plain = live(false, false).box_();
        assert!((plain.width() - 100.0).abs() < f64::EPSILON);
        assert!((plain.height() - 40.0).abs() < f64::EPSILON);

        let square = live(true, false).box_();
        assert!((square.width() - square.height()).abs() < f64::EPSILON);

        let middle = live(false, true).box_();
        assert!((middle.center() - Point::new(10.0, 10.0)).hypot() < f64::EPSILON);
        assert!((middle.width() - 200.0).abs() < f64::EPSILON);
    }
}
