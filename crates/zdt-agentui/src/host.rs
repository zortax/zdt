//! What the surface needs from whatever is around it.

use std::path::Path;
use std::rc::Rc;

use zgui::vocab::{KeyEvent, Modifiers};

/// The application the surface is inside.
///
/// Taken once, when the surface is built, and held for its life. There are no default methods: a
/// verb added later must be answered at every site, and the compiler names them.
pub trait Host {
    /// Answers something that was just asked, in one line.
    fn say(&self, said: &str);

    /// Reports something that went wrong. News, and it waits to be read.
    fn complain(&self, said: &str);

    /// Puts the editor session for `root` on screen.
    ///
    /// A thread lives in a directory, and selecting it means looking at that directory's work.
    /// `inherits` names the project whose saved editor state a fresh worktree session starts
    /// from: buffers, splits, and tree, carried over to the new checkout.
    fn open_project(&self, root: &Path, inherits: Option<&Path>);

    /// Opens `path` in the session's editor, at `line` when one is named, counting from one.
    fn open_file(&self, path: &Path, line: Option<u64>);

    /// Asks the person one line of text, then hands it to `then`. Cancelling hands nothing.
    fn ask_line(&self, title: &str, start: &str, then: Rc<dyn Fn(String)>);

    /// Gives the agent surface the keyboard.
    fn focus_agent(&self);

    /// Gives the keyboard back to the editor's panes.
    fn leave(&self);

    /// Says the surface has just taken the keyboard, which a click on a row does.
    fn took_keyboard(&self);

    /// Answers `event` from the keymap region named `region`. `true` if the key was used.
    fn key(&self, event: &KeyEvent, modifiers: Modifiers, region: &'static str) -> bool;

    /// Whether the keyboard is in the surface. Tracked.
    fn has_keyboard(&self) -> bool;

    /// Walks the current project's files and hands the relative paths to `then`.
    fn files(&self, then: Rc<dyn Fn(Vec<String>)>);

    /// The models the picker falls back to when the session has not said its own.
    fn models(&self) -> Vec<String>;

    /// The configured provider instances: each name beside its harness word, default first.
    fn instances(&self) -> Vec<(String, String)>;

    /// The directory of the session on screen, when there is one.
    fn project_root(&self) -> Option<std::path::PathBuf>;

    /// A hand on the application's picker, taken where the surrounding context still answers
    /// and usable later — from a task, an effect, an answer off the socket.
    ///
    /// Calling it offers rows — a label beside a dimmer detail — under a title, and hands the
    /// chosen index on. Cancelling hands nothing.
    fn offer(&self) -> Option<Offer>;

    /// Whether assistant prose is shown while it streams. Tracked.
    ///
    /// Off holds each message back until it is done, so half-arrived markdown is never drawn.
    fn streams_text(&self) -> bool;

    /// Whether a run of tool calls and thoughts folds into one card. Tracked.
    ///
    /// Off draws every call and every thought as a row of its own.
    fn groups_activity(&self) -> bool;
}

/// A hand on the picker: title, rows, and what to do with the chosen index.
pub type Offer = Rc<dyn Fn(&'static str, Vec<(String, String)>, Rc<dyn Fn(usize)>)>;

/// A host that is not anywhere. What a test uses.
pub struct Nowhere;

impl Host for Nowhere {
    fn say(&self, _said: &str) {}

    fn complain(&self, _said: &str) {}

    fn open_project(&self, _root: &Path, _inherits: Option<&Path>) {}

    fn open_file(&self, _path: &Path, _line: Option<u64>) {}

    fn ask_line(&self, _title: &str, _start: &str, _then: Rc<dyn Fn(String)>) {}

    fn focus_agent(&self) {}

    fn leave(&self) {}

    fn took_keyboard(&self) {}

    fn key(&self, _event: &KeyEvent, _modifiers: Modifiers, _region: &'static str) -> bool {
        false
    }

    fn has_keyboard(&self) -> bool {
        false
    }

    fn files(&self, _then: Rc<dyn Fn(Vec<String>)>) {}

    fn models(&self) -> Vec<String> {
        Vec::new()
    }

    fn instances(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    fn project_root(&self) -> Option<std::path::PathBuf> {
        None
    }

    fn offer(&self) -> Option<Offer> {
        None
    }

    fn streams_text(&self) -> bool {
        false
    }

    fn groups_activity(&self) -> bool {
        true
    }
}
