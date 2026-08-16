//! Scrolling, which moves the view and not the text.

use super::*;

impl Engine {
    /// Where the view goes, without the caret moving.
    pub(super) fn scroll(&mut self, action: &Action, count: u32) -> Step {
        let scroll = match action.leaf() {
            "center" => Scroll::Center,
            "top" => Scroll::Top,
            "bottom" => Scroll::Bottom,
            "lines" => {
                let lines = action.args.number("lines").unwrap_or(1) as i32;
                Scroll::Lines(lines * count.max(1) as i32)
            }
            other => return Step::one(Effect::Complain(format!("no scroll {other}"))),
        };
        Step::one(Effect::Scroll(scroll))
    }
}
