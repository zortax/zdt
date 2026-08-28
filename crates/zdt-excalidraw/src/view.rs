//! The editor, as a document.
//!
//! Three things stacked: the drawing, the chrome over it, and a surface above both that takes every
//! pointer event. What the surface does with a press is the pointer module's business; what it draws
//! is the overlay's.

use kurbo::{Point, Vec2};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RenderEffect, RwSignal};
use zgui::{component, view};

use crate::bar::{PropertiesProps, ToolRowProps};
use crate::layers::{LayersProps, images::Pictures};
use crate::pointer::{self, Held};
use crate::state::{Board, Tool};
use crate::text::ComposerProps;

/// What the host does with a change.
///
/// The editor never writes a file: it hands the drawing back and the host decides what that means.
#[derive(Clone)]
pub struct Sink(pub std::rc::Rc<dyn Fn(&excalidraw::Scene)>);

impl Sink {
    /// A sink that does nothing, for a host that watches the revision instead.
    #[must_use]
    pub fn none() -> Self {
        Self(std::rc::Rc::new(|_| {}))
    }
}

/// The editor over one drawing.
#[component]
pub fn Editor(
    /// The drawing, and everything about how it is being looked at.
    board: Board,
    /// What the host does with a change.
    #[prop(optional)]
    sink: Option<Sink>,
) -> impl IntoView {
    let node = NodeRef::new();
    let pictures = Pictures::new();

    // The view's own size, so the visible square and every hit test are measured against it.
    {
        let size = node.observe_content_size();
        let measuring = RenderEffect::new(move |_| {
            let measured = size.get();
            let scale = density(node.scale());
            board.viewport.set_size(
                f64::from(measured.width.0 / scale),
                f64::from(measured.height.0 / scale),
            );
        });
        on_cleanup_local(move || drop(measuring));
    }

    // The host hears about a change once, after it has been made.
    if let Some(sink) = sink {
        let telling = RenderEffect::new(move |previous: Option<u64>| {
            let at = board.revision.get();
            if previous.is_some_and(|was| was != at) {
                sink.0(&board.read_untracked());
            }
            at
        });
        on_cleanup_local(move || drop(telling));
    }

    // Pictures the drawing no longer holds are let go of.
    {
        let pictures = pictures.clone();
        let tidying = RenderEffect::new(move |_| {
            let scene = board.scene.get();
            pictures.retain(&scene.drawing.files);
        });
        on_cleanup_local(move || drop(tidying));
    }

    // Where the pointer is, in the view's own pixels. The event says where it is in the window,
    // and the difference between the two corners is everything left of and above the editor.
    let local = move |position: zgui::geom::Point<zgui::geom::CssPx, zgui::geom::Css>| -> Point {
        let scale = density(node.scale());
        let (left, top) = node
            .window_bounds()
            .map(|held| (held.origin.x.0 / scale, held.origin.y.0 / scale))
            .unwrap_or((0.0, 0.0));
        Point::new(
            f64::from(position.x.0 - left),
            f64::from(position.y.0 - top),
        )
    };

    // Whether the space bar is down, which pans under any tool.
    let space: RwSignal<bool, LocalStorage> = RwSignal::new_local(false);
    let held = move |modifiers: zgui::vocab::Modifiers, button: Option<PointerButton>| Held {
        shift: modifiers.shift(),
        alt: modifiers.alt(),
        adding: modifiers.control() || modifiers.meta(),
        space: space.get_untracked(),
        middle: button == Some(PointerButton::Middle),
    };

    view! {
        column(
            class = "exdraw",
            node_ref = node,
            tabindex = Focus::Programmatic,
            a11y:role = Role::Document,
            a11y:label = "Excalidraw drawing"
        ) {
            Scheme(board = board)
            Layers(board = board, pictures = pictures)
            Chrome(board = board)
            box(
                class = "exdraw__surface",
                attr:data-tool = move || Some(tool_word(board.tool.get()).to_owned()),
                attr:data-dragging = move || board.live.get().map(|_| "true".to_owned()),
                on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                    // The middle button is not the primary one, and it is what pans.
                    let middle = ev.button == Some(PointerButton::Middle);
                    if !ev.primary && !middle {
                        return;
                    }
                    let at = local(ev.position);
                    // A run of points is walked by pressing, not by dragging.
                    if pointer::add_point(&board, at) || board.live.get_untracked().is_some() {
                        ev.capture_pointer();
                        ev.stop_propagation();
                        return;
                    }
                    if pointer::down(&board, at, held(ev.modifiers, ev.button)) {
                        ev.capture_pointer();
                        ev.stop_propagation();
                        ev.prevent_default();
                    }
                },
                on:pointer_move = move |ev: &mut EventCx<'_, events::PointerMove>| {
                    pointer::moved(&board, local(ev.position), held(ev.modifiers, ev.button));
                },
                on:pointer_up = move |ev: &mut EventCx<'_, events::PointerUp>| {
                    ev.release_pointer();
                    pointer::up(&board);
                },
                on:pointer_cancel = move |ev: &mut EventCx<'_, events::PointerCancel>| {
                    ev.release_pointer();
                    pointer::cancel(&board);
                },
                on:pointer_leave = move |_: &mut EventCx<'_, events::PointerLeave>| {
                    // A tool that draws its own pointer has nowhere to draw it once the pointer
                    // has gone.
                    board.pointer.set(None);
                },
                on:double_click = move |ev: &mut EventCx<'_, events::DoubleClick>| {
                    // A double click finishes a run of points, and otherwise opens whatever is
                    // under it for editing.
                    if pointer::finish_points(&board) {
                        ev.stop_propagation();
                        return;
                    }
                    crate::text::open_at(&board, local(ev.position));
                    ev.stop_propagation();
                },
                on:wheel = move |ev: &mut EventCx<'_, events::Wheel>| {
                    let delta = ev.delta.to_pixels(zgui::geom::CssPx(16.0));
                    let at = local(ev.position);
                    if ev.modifiers.control() || ev.modifiers.meta() {
                        let factor = crate::viewport::Viewport::wheel_factor(
                            f64::from(delta.height.0),
                        );
                        board.viewport.zoom_by(factor, Some(at));
                    } else {
                        board.viewport.pan_by(Vec2::new(
                            f64::from(-delta.width.0),
                            f64::from(-delta.height.0),
                        ));
                    }
                    ev.prevent_default();
                    ev.stop_propagation();
                },
                on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                    if is_space(&ev.key) {
                        space.set(true);
                    }
                },
                on:key_up = move |ev: &mut EventCx<'_, events::KeyUp>| {
                    if is_space(&ev.key) {
                        space.set(false);
                    }
                }
            ) {}
            ToolRow(board = board)
            Panel(board = board)
            Composer(board = board)
            Notice(board = board)
        }
    }
}

/// What the desktop asked for, read out of the style engine.
///
/// The drawing's colours are painted rather than cascaded, so the scheme has to reach Rust — and
/// only the style engine resolves *whichever the desktop asked for*. A box the media query makes
/// one pixel wide or two carries the answer across; it is off the page and never seen.
#[component]
fn Scheme(
    /// The editor this belongs to.
    board: Board,
) -> impl IntoView {
    let probe = NodeRef::new();
    let size = probe.observe_content_size();
    let reading = RenderEffect::new(move |_| {
        let measured = size.get();
        let scale = density(probe.scale());
        let width = f64::from(measured.width.0 / scale);
        // Nothing measured yet is not an answer, so the flag is left as it was.
        if width > 0.5 {
            board.prefers_dark.set(width > 1.5);
        }
    });
    on_cleanup_local(move || drop(reading));

    view! { box(class = "exdraw__scheme", node_ref = probe, a11y:hidden = true) {} }
}

/// The chrome over the drawing: the selection, its handles, and the ghost of a pending shape.
#[component]
fn Chrome(
    /// The editor this belongs to.
    board: Board,
) -> impl IntoView {
    zgui::elements::canvas()
        .class("exdraw__overlay")
        .draw(move |cx| crate::overlay::draw(cx.scene, &board))
}

/// The properties panel, while it is out.
#[component]
fn Panel(
    /// The editor this belongs to.
    board: Board,
) -> impl IntoView {
    use zdt_view::Erase;

    view! {
        {move || {
            if board.panel.get() {
                view! { Properties(board = board) }.any()
            } else {
                ().any()
            }
        }}
    }
}

/// What the corner has to say, when it has anything.
#[component]
fn Notice(
    /// The editor this belongs to.
    board: Board,
) -> impl IntoView {
    use zdt_view::Erase;

    view! {
        {move || {
            board
                .notice
                .get()
                .map(|words| view! { label(class = "exdraw__notice") { {words} } }.any())
                .unwrap_or_else(|| ().any())
        }}
    }
}

/// Whether `key` is the space bar.
fn is_space(key: &Key) -> bool {
    matches!(key, Key::Character(held) if held == " ")
}

/// A density made safe to divide by.
fn density(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0.01 {
        scale
    } else {
        1.0
    }
}

/// The word a tool is named by in the style sheet, which is what chooses the cursor.
const fn tool_word(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => "select",
        Tool::Hand => "hand",
        Tool::Rectangle => "rectangle",
        Tool::Diamond => "diamond",
        Tool::Ellipse => "ellipse",
        Tool::Arrow => "arrow",
        Tool::Line => "line",
        Tool::Freedraw => "freedraw",
        Tool::Text => "text",
        Tool::Image => "image",
        Tool::Frame => "frame",
        Tool::Eraser => "eraser",
    }
}
