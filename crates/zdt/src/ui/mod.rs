//! Everything the window is drawn from.

pub mod chrome;
pub mod cmdline;
pub mod diagnostics;
pub mod frame;
pub mod hover;
pub mod leap;
pub mod pane;
pub mod panes;
pub mod picker;
pub mod prompt;
pub mod spinner;
pub mod statusline;
pub mod terminal;
pub mod theme;
pub mod tree;
pub mod treemenu;
pub mod whichkey;

use zgui::prelude::*;

/// Erases a view's type.
///
/// Two branches of a choice build different views, and a reactive hole needs them to be one
/// type. This is the conversion, as a method, so a branch reads as the view it is with `.any()`
/// on the end rather than as a call wrapped around it.
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
/// Every surface that comes and goes — a picker, a menu, the floating terminal — is wrapped in a
/// presence so that it stays mounted for the length of its exit animation, and binds this to
/// `data-state` so that `assets/css/motion.css` has something to select the exit on. Read inside
/// a tracked closure: the value changes under the surface, once, on the way out.
///
/// `None` outside a presence, which is a surface nothing is animating and a state nothing needs.
#[must_use]
pub fn leaving_state(presence: Option<zgui_ui_primitives::PresenceContext>) -> Option<String> {
    presence.map(|presence| presence.state_name().to_owned())
}
