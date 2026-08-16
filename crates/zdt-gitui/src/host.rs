//! What the panel needs from whatever is around it.
//!
//! Everything the panel does that is not git is here. Seven verbs, none of which it can answer on
//! its own. Inside an editor they are the workspace, the announcements and the modal layer. On
//! their own they are nothing, which is what [`Nowhere`] is.

use std::path::Path;

use zgui::vocab::{KeyEvent, Modifiers};

/// The application the panel is inside.
///
/// Taken once, when the panel is built, and held for its life. Every one of the panel's operations
/// reports from inside a task, and a context looked up after an await is gone.
///
/// There are no default methods. A verb added later must be answered at every site, and the
/// compiler names them.
pub trait Host {
    /// Answers something that was just asked, in one line.
    ///
    /// "nothing is staged", "this project is not in a git repository". Worth as long as it takes
    /// to read.
    fn say(&self, said: &str);

    /// Reports something that went wrong.
    ///
    /// News, and not an answer. It has to wait to be read.
    fn complain(&self, said: &str);

    /// Opens `path` for editing. The panel has already put itself away.
    fn open(&self, path: &Path);

    /// Shows the panel as a tab, when there is anywhere to put one.
    fn open_as_tab(&self);

    /// Gives the keyboard back to whatever had it before the panel took it.
    fn release_keyboard(&self);

    /// Answers `event` from the panel's keymap region. `true` if the key was used.
    ///
    /// What a chord is, and which file binds it, belongs to the host. The panel needs only to know
    /// whether to let the key through.
    fn key(&self, event: &KeyEvent, modifiers: Modifiers) -> bool;

    /// Whether the panel's own pane is the one being looked at.
    ///
    /// Read inside a render effect. An implementation must read *tracked*: an answer that is
    /// cached, or read untracked, is a panel that never takes the keyboard when its tab becomes
    /// the current one.
    fn is_in_front(&self) -> bool;
}

/// A host that is not anywhere.
///
/// It says nothing, opens nothing, and answers no keys. What a test uses, and where a window with
/// only a git panel in it starts.
pub struct Nowhere;

impl Host for Nowhere {
    fn say(&self, _said: &str) {}

    fn complain(&self, _said: &str) {}

    fn open(&self, _path: &Path) {}

    fn open_as_tab(&self) {}

    fn release_keyboard(&self) {}

    fn key(&self, _event: &KeyEvent, _modifiers: Modifiers) -> bool {
        false
    }

    fn is_in_front(&self) -> bool {
        false
    }
}
