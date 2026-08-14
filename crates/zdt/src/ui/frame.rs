//! The window's own frame.
//!
//! The desktop draws nothing around this window, so everything a title bar would have given the
//! user is owed by the application. None of it can be done by moving the window from inside: a
//! Wayland compositor never lets a window place itself, so each affordance asks the desktop to
//! take over the gesture instead.
//!
//! What that comes to is one handler on the header strip, eight invisible strips around the edge,
//! and a rule that a control inside the header must stop the press before the strip behind it
//! sees it — otherwise the click a button is waiting for is never formed, because once a drag
//! begins the compositor owns the pointer and no release ever arrives.

use zgui::prelude::*;
use zgui::{component, view};

use crate::icons::{self, IconProps};

/// The window: its three rows, and the eight edges around them.
///
/// The children are the rows, in order — the header, the body, the status line. They are one
/// slot rather than three because the frame does not care what is in them: it owns the corners,
/// the edges and the background, and nothing else.
#[component]
pub fn Frame(
    /// The rows of the window.
    children: Children,
) -> impl IntoView {
    let window = use_window();
    // A window filling the screen has no corners to round and no edge to drag: a rounded corner
    // there is a notch cut out of the desktop.
    let flush = {
        let window = window.clone();
        move || window.maximized().get() || window.fullscreen().get().is_some()
    };

    view! {
        column(class = "frame", attr:data-flush = move || flush().then(|| "true".to_owned())) {
            {children.into_view_once()}

            box(class = "edges") {
                Grip(edge = ResizeEdge::North, class = "edge edge--n")
                Grip(edge = ResizeEdge::South, class = "edge edge--s")
                Grip(edge = ResizeEdge::West, class = "edge edge--w")
                Grip(edge = ResizeEdge::East, class = "edge edge--e")
                Grip(edge = ResizeEdge::NorthWest, class = "edge edge--nw")
                Grip(edge = ResizeEdge::NorthEast, class = "edge edge--ne")
                Grip(edge = ResizeEdge::SouthWest, class = "edge edge--sw")
                Grip(edge = ResizeEdge::SouthEast, class = "edge edge--se")
            }
        }
    }
}

/// One edge or corner the window can be resized from.
#[component]
fn Grip(
    /// Which side of the window this is.
    edge: ResizeEdge,
    /// Where it sits, and how large it is.
    class: &'static str,
) -> impl IntoView {
    let window = use_window();
    let cursor = cursor_for(edge);
    // Each grip reads this for itself rather than being handed it: reading a signal is what
    // subscribes to it, and a grip that subscribes is one that appears and disappears on its own.
    let framed = {
        let window = window.clone();
        move || !window.maximized().get() && window.fullscreen().get().is_none()
    };

    view! {
        Show(when = framed) {
            box(
                class = class,
                on:pointer_down = window.resize_drag_handler(edge),
                // Imperative rather than from the sheet: the cursor is not a style this engine
                // reads, and setting it on the press would change it after the drag had begun.
                on:pointer_enter = {
                    let window = window.clone();
                    move |_| window.set_cursor(cursor)
                },
                on:pointer_leave = {
                    let window = window.clone();
                    move |_| window.set_cursor(CursorStyle::Default)
                }
            ) {}
        }
    }
}

/// What the pointer should look like over one edge.
const fn cursor_for(edge: ResizeEdge) -> CursorStyle {
    match edge {
        ResizeEdge::North | ResizeEdge::South => CursorStyle::ResizeNorthSouth,
        ResizeEdge::East | ResizeEdge::West => CursorStyle::ResizeEastWest,
        ResizeEdge::NorthWest | ResizeEdge::SouthEast => CursorStyle::ResizeNorthWestSouthEast,
        _ => CursorStyle::ResizeNorthEastSouthWest,
    }
}

/// Minimise, maximise and close, at the end of the header strip.
///
/// Each button stops the press reaching the strip behind it, which is what makes it a button
/// rather than a place the window happens to be dragged from.
#[component]
pub fn WindowControls() -> impl IntoView {
    let window = use_window();
    let restore = {
        let window = window.clone();
        move || {
            if window.maximized().get() {
                icons::COPY
            } else {
                icons::SQUARE
            }
        }
    };

    view! {
        row(class = "wincontrols") {
            control(
                class = "wincontrol",
                tabindex = Focus::Programmatic,
                a11y:label = "Minimise",
                on:pointer_down = window.no_drag_handler(),
                on:click = {
                    let window = window.clone();
                    move |_| window.minimize()
                }
            ) {
                Icon(icon = icons::MINUS, class = "icon--sm")
            }
            control(
                class = "wincontrol",
                tabindex = Focus::Programmatic,
                a11y:label = "Maximise",
                on:pointer_down = window.no_drag_handler(),
                on:click = {
                    let window = window.clone();
                    move |_| window.toggle_maximized()
                }
            ) {
                Icon(icon = Signal::derive_local(restore), class = "icon--sm")
            }
            control(
                class = "wincontrol wincontrol--close",
                tabindex = Focus::Programmatic,
                a11y:label = "Close",
                on:pointer_down = window.no_drag_handler(),
                on:click = {
                    let window = window.clone();
                    move |_| window.close()
                }
            ) {
                Icon(icon = icons::X, class = "icon--sm")
            }
        }
    }
}
