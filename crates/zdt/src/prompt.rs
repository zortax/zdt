//! Asking for one line of text.
//!
//! A new file's name, a rename, `:` later, an LSP rename after that. All of them are the same
//! thing — a title, a starting value, and something to do with what was typed — so all of them
//! share one floating input rather than growing a dialog each.
//!
//! Only one can be open at a time, which matches how it is used: the keyboard is in it, so there
//! is no way to start a second.

use std::rc::Rc;

use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

/// A question waiting for an answer.
#[derive(Clone)]
pub struct Pending {
    /// What is being asked, shown above the input.
    pub title: String,
    /// What the input starts out holding.
    pub start: String,
    /// What to do with the answer. Not called when the prompt is cancelled.
    pub answer: Rc<dyn Fn(&str)>,
}

impl PartialEq for Pending {
    fn eq(&self, other: &Self) -> bool {
        // The callback is not comparable, and two prompts asking the same thing are the same
        // prompt as far as anything drawing one is concerned.
        self.title == other.title && self.start == other.start
    }
}

/// The one prompt.
#[derive(Clone, Copy)]
pub struct Prompt {
    pending: RwSignal<Option<Pending>, LocalStorage>,
}

impl Prompt {
    /// A prompt with nothing being asked.
    #[must_use]
    pub fn new() -> Self {
        Self {
            pending: RwSignal::new_local(None),
        }
    }

    /// Asks for a line of text.
    pub fn ask(
        &self,
        title: impl Into<String>,
        start: impl Into<String>,
        answer: impl Fn(&str) + 'static,
    ) {
        self.pending.set(Some(Pending {
            title: title.into(),
            start: start.into(),
            answer: Rc::new(answer),
        }));
    }

    /// What is being asked, if anything. Tracked.
    #[must_use]
    pub fn pending(&self) -> Option<Pending> {
        self.pending.get()
    }

    /// Whether something is being asked. Tracked.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.pending.with(Option::is_some)
    }

    /// Answers, and closes.
    pub fn submit(&self, text: &str) {
        let Some(pending) = self.pending.get_untracked() else {
            return;
        };
        self.pending.set(None);
        let text = text.trim();
        if !text.is_empty() {
            (pending.answer)(text);
        }
    }

    /// Closes without answering.
    pub fn cancel(&self) {
        if self.pending.with_untracked(Option::is_some) {
            self.pending.set(None);
        }
    }
}

impl Default for Prompt {
    fn default() -> Self {
        Self::new()
    }
}

/// Puts the prompt where every component can find it.
pub fn provide(prompt: Prompt) {
    zgui::reactive::provide_local_context(prompt);
}

/// The prompt, from inside a component.
///
/// # Panics
///
/// If none was provided above this component, which is a wiring mistake.
#[must_use]
pub fn use_prompt() -> Prompt {
    zgui::reactive::use_local_context::<Prompt>().expect("a prompt is provided at the root")
}
