//! Making two branches one type, and reading an exit animation.

use zgui::prelude::*;

/// Erases a view's type.
///
/// Two branches of a choice build different views, and a reactive hole needs them to be one type.
/// This is the conversion as a method, so a branch reads as the view it is with `.any()` on the
/// end.
pub trait Erase {
    /// This view, with its type forgotten.
    fn any(self) -> zgui::view::AnyView;
}

impl<V: IntoView> Erase for V {
    fn any(self) -> zgui::view::AnyView {
        zgui::view::AnyView::new(self)
    }
}

/// What the enclosing [`Presence`](zgui_ui_primitives::Presence) is doing, as an attribute value.
///
/// Every surface that comes and goes is wrapped in a presence. The presence keeps it mounted for
/// the length of its exit animation. Bind this to `data-state` to give a style sheet something to
/// select the exit on. Read it inside a tracked closure: the value changes under the surface,
/// once, on the way out.
///
/// `None` outside a presence. That is a surface nothing animates, and a state nothing needs.
#[must_use]
pub fn leaving_state(presence: Option<zgui_ui_primitives::PresenceContext>) -> Option<String> {
    presence.map(|presence| presence.state_name().to_owned())
}
