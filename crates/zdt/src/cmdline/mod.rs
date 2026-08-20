//! The command line's state, and what its commands do.
//!
//! [`zdt_vim::ex`] reads a line into a description of what was asked for. This is the other half:
//! the text being typed, the history behind it, and the code that carries each description out
//! against the workspace.
//!
//! Everything a command does is something a key can also do. `:w` and `<Leader>w` reach the same
//! save, and `:sp` and `<C-w>s` the same split. That is deliberate. There are two ways to ask for
//! one thing, and one implementation.

pub mod view;

mod run;

use std::cell::RefCell;
use std::rc::Rc;

use zdt_vim::ex::{BufferTarget, Command, Range};
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

use crate::workspace::Workspace;

/// How many lines of history to keep.
///
/// A session's worth. This is not a shell and there is no history file to manage.
const HISTORY: usize = 200;

/// The command line.
#[derive(Clone)]
pub struct CommandLine {
    inner: Rc<Inner>,
}

struct Inner {
    workspace: Workspace,
    /// Whether one is being typed.
    open: RwSignal<bool, LocalStorage>,
    /// What has been typed.
    text: RwSignal<String, LocalStorage>,
    /// What was typed before, most recent last.
    history: RefCell<Vec<String>>,
    /// How far back through it the arrows have walked.
    walked: RefCell<Option<usize>>,
}

impl CommandLine {
    /// A command line with nothing being typed.
    #[must_use]
    pub fn new(workspace: Workspace) -> Self {
        Self {
            inner: Rc::new(Inner {
                workspace,
                open: RwSignal::new_local(false),
                text: RwSignal::new_local(String::new()),
                history: RefCell::new(Vec::new()),
                walked: RefCell::new(None),
            }),
        }
    }

    /// Whether one is being typed. Tracked.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.inner.open.get()
    }

    /// What has been typed. Tracked.
    #[must_use]
    pub fn text(&self) -> String {
        self.inner.text.get()
    }

    /// Opens one, starting with `start`.
    ///
    /// `:` opens an empty one; `:'<,'>` is what a visual selection opens, so the range is already
    /// there when the command is typed.
    pub fn open(&self, start: &str) {
        self.inner.text.set(start.to_owned());
        *self.inner.walked.borrow_mut() = None;
        self.inner.open.set(true);
    }

    /// What was typed before, oldest first.
    #[must_use]
    pub fn history(&self) -> Vec<String> {
        self.inner.history.borrow().clone()
    }

    /// Puts a history back, which restoring a session does.
    pub fn restore_history(&self, history: Vec<String>) {
        *self.inner.history.borrow_mut() = history;
        *self.inner.walked.borrow_mut() = None;
    }

    /// Puts `text` in it, which typing and walking the history both do.
    pub fn set_text(&self, text: &str) {
        if self.inner.text.with_untracked(|held| held != text) {
            self.inner.text.set(text.to_owned());
        }
    }

    /// Closes it without running anything.
    pub fn cancel(&self) {
        self.close();
    }

    /// One step back through the history, if there is one.
    pub fn older(&self) -> Option<String> {
        let history = self.inner.history.borrow();
        if history.is_empty() {
            return None;
        }
        let mut walked = self.inner.walked.borrow_mut();
        let next = match *walked {
            Some(0) => 0,
            Some(at) => at - 1,
            None => history.len() - 1,
        };
        *walked = Some(next);
        history.get(next).cloned()
    }

    /// One step forward. It answers nothing at the end, which clears the line.
    pub fn newer(&self) -> Option<String> {
        let history = self.inner.history.borrow();
        let mut walked = self.inner.walked.borrow_mut();
        let at = (*walked)?;
        if at + 1 >= history.len() {
            *walked = None;
            return None;
        }
        *walked = Some(at + 1);
        history.get(at + 1).cloned()
    }

    /// Runs what has been typed, and closes.
    pub fn submit(&self) {
        let line = self.inner.text.get_untracked();
        self.close();

        let trimmed = line.trim();
        if trimmed.is_empty() {
            return;
        }
        {
            let mut history = self.inner.history.borrow_mut();
            // The same command twice in a row is one line of history, not two.
            if history.last().map(String::as_str) != Some(trimmed) {
                history.push(trimmed.to_owned());
                if history.len() > HISTORY {
                    history.remove(0);
                }
            }
        }

        if let Some(command) = zdt_vim::ex::parse(trimmed) {
            self.run(command);
        }
    }

    /// Closes it and gives the keyboard back.
    fn close(&self) {
        if self.inner.open.get_untracked() {
            self.inner.open.set(false);
            self.inner.text.set(String::new());
        }
    }
}

/// Where `pattern` occurs in `text`, as byte ranges.
///
/// Literal, not a regular expression: what people type into `:s` is a word far more often than a
/// pattern, and a half-supported regular expression is worse than an honest literal one.
fn matches_in(
    text: &str,
    pattern: &str,
    all: bool,
    ignore_case: bool,
) -> Vec<std::ops::Range<usize>> {
    let (haystack, needle) = if ignore_case {
        (text.to_lowercase(), pattern.to_lowercase())
    } else {
        (text.to_owned(), pattern.to_owned())
    };
    // Lowercasing can change a string's length, which would put every offset after it wrong.
    let (haystack, needle) = if haystack.len() == text.len() {
        (haystack, needle)
    } else {
        (text.to_owned(), pattern.to_owned())
    };

    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = haystack[from..].find(&needle) {
        let start = from + at;
        found.push(start..start + needle.len());
        if !all {
            break;
        }
        from = start + needle.len().max(1);
        if from > haystack.len() {
            break;
        }
    }
    found
}

/// Puts the command line where every component can find it.
pub fn provide(cmdline: CommandLine) {
    zgui::reactive::provide_local_context(cmdline);
}

/// It, from inside a component.
///
/// # Panics
///
/// If none was provided above this component, which is a wiring mistake.
#[must_use]
pub fn use_cmdline() -> CommandLine {
    zgui::reactive::use_local_context::<CommandLine>().expect("a command line is at the root")
}

#[cfg(test)]
mod tests {
    use super::matches_in;

    #[test]
    fn only_the_first_unless_g_was_asked_for() {
        assert_eq!(matches_in("a a a", "a", false, false), vec![0..1]);
        assert_eq!(
            matches_in("a a a", "a", true, false),
            vec![0..1, 2..3, 4..5]
        );
    }

    #[test]
    fn case_is_ignored_only_when_it_is_asked_for() {
        assert!(matches_in("Alpha", "alpha", true, false).is_empty());
        assert_eq!(matches_in("Alpha", "alpha", true, true), vec![0..5]);
    }

    #[test]
    fn overlapping_text_advances_past_what_it_replaced() {
        // `aa` in `aaaa` is two matches, not three: a substitution replaces what it matched.
        assert_eq!(matches_in("aaaa", "aa", true, false), vec![0..2, 2..4]);
    }

    #[test]
    fn nothing_to_find_is_no_matches() {
        assert!(matches_in("hello", "zzz", true, false).is_empty());
        assert!(matches_in("", "a", true, false).is_empty());
    }
}
