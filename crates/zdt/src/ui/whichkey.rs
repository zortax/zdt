//! What could come next, shown after a pause.
//!
//! A panel across the bottom listing the keys that would continue what has been typed, with what
//! each of them leads to. It appears only after a delay, so somebody who knows the sequence never
//! sees it and somebody who does not is never left guessing.
//!
//! # Why it takes no focus
//!
//! It is an ordinary box in the frame rather than an overlay: an overlay would trap focus, and a
//! panel that took the keyboard away from the editor in the middle of a key sequence would break
//! the very sequence it is there to help with.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RenderEffect, RwSignal};
use zgui::view::time::{TimeoutHandle, Timers};
use zgui::{component, view};

use crate::vim::{Continuation, use_vim};

/// How long a sequence has to sit before the panel appears.
const DELAY: Duration = Duration::from_millis(300);

/// The panel.
#[component]
pub fn WhichKey() -> impl IntoView {
    let vim = use_vim();

    // What is shown, which is not what is pending: the delay sits between them.
    let shown: RwSignal<Option<(String, Vec<Continuation>)>, LocalStorage> =
        RwSignal::new_local(None);

    // The timer is armed from the reactive flush, which has no window scope of its own.
    let timers = Timers::current();
    let waiting: Rc<RefCell<Option<TimeoutHandle>>> = Rc::new(RefCell::new(None));

    let watching = {
        let vim = vim.clone();
        let waiting = Rc::clone(&waiting);
        RenderEffect::new(move |_| {
            let pending = vim.pending();
            if pending.is_empty() {
                // The sequence resolved or was abandoned: the panel goes at once rather than
                // waiting for its delay to run out.
                *waiting.borrow_mut() = None;
                if shown.get_untracked().is_some() {
                    shown.set(None);
                }
                return;
            }

            // Already showing: the panel follows the sequence with no second delay, which is what
            // makes walking down a group feel like one panel rather than a flicker of several.
            if shown.get_untracked().is_some() {
                let next = vim.continuations();
                if next.is_empty() {
                    shown.set(None);
                } else {
                    shown.set(Some((pending, next)));
                }
                return;
            }

            let Some(timers) = timers.as_ref() else {
                return;
            };
            let vim = vim.clone();
            *waiting.borrow_mut() = Some(timers.set_timeout(DELAY, move || {
                let pending = vim.pending();
                let next = vim.continuations();
                if !pending.is_empty() && !next.is_empty() {
                    shown.set(Some((pending, next)));
                }
            }));
        })
    };
    on_cleanup_local(move || {
        drop(watching);
        drop(waiting);
    });

    view! {
        Show(when = move || shown.get().is_some()) {
            column(class = "whichkey", a11y:role = Role::Group, a11y:label = "Keys") {
                row(class = "whichkey__head") {
                    label(class = "whichkey__typed") {
                        {move || shown.get().map(|(keys, _)| keys).unwrap_or_default()}
                    }
                    label(class = "muted") {
                        {move || {
                            let count = shown.get().map_or(0, |(_, next)| next.len());
                            format!("{count} keys")
                        }}
                    }
                }
                box(class = "whichkey__keys") {
                    for entry in move || shown.get().map(|(_, next)| next).unwrap_or_default(),
                        key = |entry: &Continuation| entry.keys.clone()
                    {
                        Entry(entry = entry)
                    }
                }
            }
        }
    }
}

/// One key and what it leads to.
#[component]
fn Entry(
    /// The key and its label.
    entry: Continuation,
) -> impl IntoView {
    let Continuation { keys, label, runs } = entry;
    view! {
        row(
            class = "whichkey__entry",
            // A group is told from a binding by its mark rather than by its colour, so the
            // difference survives a theme that tints them the same.
            attr:data-group = (!runs).then(|| "true".to_owned())
        ) {
            label(class = "whichkey__key") {{keys}}
            label(class = "whichkey__arrow") {{if runs { "\u{2192}" } else { "\u{2192}+" }}}
            label(class = "whichkey__label nowrap") {{label}}
        }
    }
}
