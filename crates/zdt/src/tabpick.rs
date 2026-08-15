//! Choosing a tab by pressing the key on it.
//!
//! `<Leader>bb` puts a letter on every tab; the next key goes to that tab. `<Leader>bd` does the
//! same and closes the one chosen.
//!
//! This is leap for the buffer line, and it is here rather than in the picker for the same reason
//! leap is not a picker: the tabs are already on screen with their names on them, so a modal that
//! covers them to list them again is a worse way to do the same thing. Two keystrokes, no modal,
//! and the eye never leaves the row it was already reading.

use std::cell::RefCell;
use std::rc::Rc;

use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

use crate::workspace::{BufferId, Workspace};

/// The keys tabs are labelled with, in the order they are handed out.
///
/// The home row first, and no `q` — the key people press to get out of things.
const ALPHABET: &str = "asdfghjklwertyuiopzxcvbnm";

/// What choosing a tab should do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Then {
    /// Show it.
    Show,
    /// Close it.
    Close,
}

/// The labels on the buffer line, when there are any.
#[derive(Clone)]
pub struct TabPick {
    inner: Rc<Inner>,
}

struct Inner {
    workspace: Workspace,
    /// Which key labels which buffer, in buffer-line order.
    labels: RwSignal<Vec<(char, BufferId)>, LocalStorage>,
    /// What the next key will do.
    then: RefCell<Then>,
}

impl TabPick {
    /// No labels.
    #[must_use]
    pub fn new(workspace: Workspace) -> Self {
        Self {
            inner: Rc::new(Inner {
                workspace,
                labels: RwSignal::new_local(Vec::new()),
                then: RefCell::new(Then::Show),
            }),
        }
    }

    /// Puts a label on every tab. The next key chooses one.
    pub fn start(&self, then: Then) {
        let labels: Vec<(char, BufferId)> = self
            .inner
            .workspace
            .order()
            .into_iter()
            .zip(ALPHABET.chars())
            .map(|(buffer, key)| (key, buffer))
            .collect();

        if labels.is_empty() {
            self.inner.workspace.say("nothing is open");
            return;
        }
        // One tab is not a choice. Going straight there saves the keystroke, and for a close it
        // saves asking which of the one.
        if let [(_, only)] = labels[..] {
            self.choose_buffer(only, then);
            return;
        }

        *self.inner.then.borrow_mut() = then;
        self.inner.labels.set(labels);
    }

    /// Takes the labels off.
    pub fn stop(&self) {
        if !self.inner.labels.with_untracked(Vec::is_empty) {
            self.inner.labels.set(Vec::new());
        }
    }

    /// Whether tabs are labelled, without subscribing.
    #[must_use]
    pub fn is_running(&self) -> bool {
        !self.inner.labels.with_untracked(Vec::is_empty)
    }

    /// Which key is on which tab. Tracked.
    #[must_use]
    pub fn labels(&self) -> Vec<(char, BufferId)> {
        self.inner.labels.get()
    }

    /// The key on `buffer`, when it has one. Tracked.
    #[must_use]
    pub fn label_for(&self, buffer: BufferId) -> Option<char> {
        self.inner.labels.with(|labels| {
            labels
                .iter()
                .find(|(_, held)| *held == buffer)
                .map(|(key, _)| *key)
        })
    }

    /// Takes one key while the labels are up.
    ///
    /// Always answers `true`: every key belongs to this while it is running, the ones that end it
    /// included. A label that means nothing ends it rather than being ignored, because otherwise
    /// a mistyped key would eat the one after it too.
    pub fn key(&self, character: Option<char>) -> bool {
        let then = *self.inner.then.borrow();
        let chosen = character.and_then(|character| {
            self.inner.labels.with_untracked(|labels| {
                labels
                    .iter()
                    .find(|(key, _)| *key == character)
                    .map(|(_, buffer)| *buffer)
            })
        });
        self.stop();

        if let Some(buffer) = chosen {
            self.choose_buffer(buffer, then);
        }
        true
    }

    /// Shows or closes one.
    fn choose_buffer(&self, buffer: BufferId, then: Then) {
        match then {
            Then::Show => self.inner.workspace.show(buffer),
            Then::Close => {
                let dirty = self
                    .inner
                    .workspace
                    .buffer_untracked(buffer)
                    .is_some_and(|entry| entry.is_dirty());
                if dirty {
                    self.inner.workspace.show(buffer);
                    self.inner
                        .workspace
                        .complain("unsaved changes; <Leader>C closes anyway");
                } else {
                    self.inner.workspace.close_buffer(buffer);
                }
            }
        }
    }
}

/// Puts it where every component can find it.
pub fn provide(picker: TabPick) {
    zgui::reactive::provide_local_context(picker);
}

/// It, from inside a component.
///
/// # Panics
///
/// If none was provided above this component, which is a wiring mistake.
#[must_use]
pub fn use_tabpick() -> TabPick {
    zgui::reactive::use_local_context::<TabPick>().expect("a tab picker is provided at the root")
}
