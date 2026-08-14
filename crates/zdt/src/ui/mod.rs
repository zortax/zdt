//! Everything the window is drawn from.

pub mod chrome;
pub mod frame;
pub mod pane;
pub mod panes;
pub mod statusline;
pub mod theme;

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
