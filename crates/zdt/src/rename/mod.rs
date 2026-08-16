//! Renaming a symbol, over the symbol.
//!
//! A one-line box where the word is, and no prompt in the middle of the window. The difference
//! matters. A rename is about a particular word, and a box somewhere else makes somebody hold that
//! word in their head while they type its replacement.
//!
//! # Why it is an overlay
//!
//! The editor cannot host a widget inside the text. There is no virtual text, no inline widget,
//! and every line is exactly one line high. So the box floats over the symbol, and the symbol is
//! banded underneath it in its own decoration layer. The band says *this* is the thing being
//! renamed. It earns its place: without it, a box that has flipped above a symbol near the bottom
//! of the window has no visible connection to anything.

mod field;

pub use crate::rename::field::{RenameBox, RenameBoxProps};

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

/// The layer the symbol being renamed is banded in.
///
/// Its own, beside the diagnostics' and the git signs', so that opening the box does not clear
/// either and closing it does not have to put them back.
const LAYER: &str = "rename";

/// What is being renamed.
#[derive(Clone, PartialEq, Debug)]
pub struct Asking {
    /// What it is called now, which is what the box opens holding.
    pub name: String,
    /// Which bytes of the buffer it is.
    pub range: std::ops::Range<usize>,
    /// Where that is on the window.
    pub caret: zgui_editor::CaretRect,
}

/// The rename box.
#[derive(Clone, Copy)]
pub struct Rename {
    asking: RwSignal<Option<Asking>, LocalStorage>,
}

impl Rename {
    /// Nothing being renamed.
    #[must_use]
    pub fn new() -> Self {
        Self {
            asking: RwSignal::new_local(None),
        }
    }

    /// Opens the box over `range`, holding `name`.
    pub fn open(&self, name: &str, range: std::ops::Range<usize>, caret: zgui_editor::CaretRect) {
        self.asking.set(Some(Asking {
            name: name.to_owned(),
            range,
            caret,
        }));
    }

    /// Puts it away, leaving the buffer as it was.
    pub fn cancel(&self) {
        if self.asking.with_untracked(Option::is_some) {
            self.asking.set(None);
        }
    }

    /// Whether it is open, without subscribing.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.asking.with_untracked(Option::is_some)
    }

    /// What is being renamed. Tracked.
    #[must_use]
    pub fn asking(&self) -> Option<Asking> {
        self.asking.get()
    }

    /// Renames to `to` and closes.
    ///
    /// A name that has not changed is a rename nobody asked for, and running one would be a
    /// round trip and a "changed 9 files" for pressing `<CR>` by accident.
    pub fn submit(&self, workspace: &crate::workspace::Workspace, to: &str) {
        let was = self.asking.get_untracked();
        self.cancel();
        let to = to.trim();
        match was {
            Some(was) if to.is_empty() || to == was.name => {}
            Some(_) => crate::actions::lsp::rename_to(workspace, to),
            None => {}
        }
    }
}

impl Default for Rename {
    fn default() -> Self {
        Self::new()
    }
}

/// Puts the rename box where every component can find it.
pub fn provide(rename: Rename) {
    zgui::reactive::provide_local_context(rename);
}

/// It, from inside a component.
///
/// # Panics
///
/// If none was provided above this component, which is a wiring mistake.
#[must_use]
pub fn use_rename() -> Rename {
    zgui::reactive::use_local_context::<Rename>().expect("a rename box is provided at the root")
}
