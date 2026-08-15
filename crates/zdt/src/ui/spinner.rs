//! A turning spinner.
//!
//! Braille, because it turns in one cell: eight dots that can be lit in any combination give a
//! smooth wheel in the width of a character, and every nerd font has them. A glyph swapped on a
//! timer rather than anything animated by the renderer — a spinner is eight strings.
//!
//! It stops when nothing is spinning. A timer running behind a hidden spinner is a frame asked
//! for ten times a second for ever.

use std::time::Duration;

use zgui::prelude::*;
use zgui::{component, view};

/// The frames, in order.
const FRAMES: &[&str] = &[
    "\u{280b}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283c}", "\u{2834}", "\u{2826}", "\u{2827}",
    "\u{2807}", "\u{280f}",
];

/// How long each frame lasts.
///
/// Ten a second: fast enough to read as turning, slow enough that it is not a strobe and not a
/// frame the renderer has to produce sixty times a second for a single character.
const FRAME: Duration = Duration::from_millis(100);

/// One spinner, turning for as long as it is on screen.
#[component]
pub fn Spinner() -> impl IntoView {
    let at = RwSignal::new_local(0_usize);

    // Held for the component's life: dropping an interval handle is what stops it, so a spinner
    // that goes away takes its timer with it.
    let turning = zgui::view::time::Timers::current().map(|timers| {
        timers.set_interval(FRAME, move || {
            at.update(|held| *held = (*held + 1) % FRAMES.len());
        })
    });
    on_cleanup_local(move || drop(turning));

    view! {
        label(class = "spinner") {
            {move || FRAMES[at.get() % FRAMES.len()].to_owned()}
        }
    }
}
