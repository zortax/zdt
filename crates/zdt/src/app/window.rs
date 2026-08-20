//! How a window is asked for.
//!
//! One place, because the attributes that make a zdt window a zdt window do not carry from the
//! first one to the next. `App` passes only its stylesheet down to a window opened later, so a
//! second window asked for without these draws the desktop's own title bar over a frame that has
//! already drawn its own.

use zgui::platform::Decorations;
use zgui::runtime::windows::WindowOptions;

/// How wide and how tall a window opens.
const SIZE: (f32, f32) = (1280.0, 800.0);
/// And how small it may be made.
const LEAST: (f32, f32) = (480.0, 320.0);

/// The attributes every zdt window has.
///
/// The two that matter are the decorations and the transparency: `assets/css/frame.css` draws the
/// title bar, the corners and the resize grips, and it can only do that on a window the desktop
/// has not already drawn on.
#[must_use]
pub fn options(title: &str) -> WindowOptions {
    WindowOptions::new(title)
        .with_size(SIZE.0, SIZE.1)
        .with_min_size(LEAST.0, LEAST.1)
        .with_decorations(Decorations::None)
        .with_transparent(true)
}

/// What a window showing `name` is called.
#[must_use]
pub fn title_for(name: &str) -> String {
    format!("{name} \u{2014} zdt")
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_title_names_the_session_first() {
        // The session's name first, because that is what a person is picking between in a task
        // switcher that has truncated everything after the first few characters.
        assert_eq!(super::title_for("zdt"), "zdt \u{2014} zdt");
    }
}
