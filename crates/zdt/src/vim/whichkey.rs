//! What could come next, shown after a pause.
//!
//! A panel across the bottom listing the keys that would continue what has been typed, with what
//! each of them leads to. It appears only after a delay, so somebody who knows the sequence never
//! sees it and somebody who does not is never left guessing.
//!
//! # Why it takes no focus
//!
//! It is an ordinary box in the frame, and no overlay. An overlay would trap focus, and a panel
//! that took the keyboard away from the editor in the middle of a key sequence would break the
//! very sequence it is there to help with.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RenderEffect, RwSignal};
use zgui::view::time::{TimeoutHandle, Timers};
use zgui::{component, view};

use crate::settings::use_settings;
use crate::vim::{Continuation, use_vim};

/// The panel.
#[component]
pub fn WhichKey() -> impl IntoView {
    let vim = use_vim();
    let settings = use_settings();

    // What is shown, which is not what is pending: the delay sits between them.
    let shown: RwSignal<Option<(String, Vec<Continuation>)>, LocalStorage> =
        RwSignal::new_local(None);

    // The timer is armed from the reactive flush, which has no window scope of its own.
    let timers = Timers::current();
    let waiting: Rc<RefCell<Option<TimeoutHandle>>> = Rc::new(RefCell::new(None));

    let watching = {
        let vim = vim.clone();
        let settings = settings.clone();
        let waiting = Rc::clone(&waiting);
        RenderEffect::new(move |_| {
            let pending = vim.pending();
            if pending.is_empty() {
                // The sequence resolved or was abandoned, so the panel goes at once. Its delay
                // is for appearing, and never for leaving.
                *waiting.borrow_mut() = None;
                if shown.get_untracked().is_some() {
                    shown.set(None);
                }
                return;
            }

            // Already showing. The panel follows the sequence with no second delay, so walking
            // down a group feels like one panel.
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
            let delay = Duration::from_millis(settings.with(|config| config.ui.whichkey_delay));
            let vim = vim.clone();
            *waiting.borrow_mut() = Some(timers.set_timeout(delay, move || {
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
                    // Keyed by the whole row, and never by its key alone. A row's label is a
                    // construction-time value, so a row reused across a change keeps the label it
                    // was built with. Walking from the leader map into a group shares plenty of
                    // keys with it.
                    for entry in move || shown.get().map(|(_, next)| next).unwrap_or_default(),
                        key = |entry: &Continuation| (entry.keys.clone(), entry.label.clone())
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
            // A group is told from a binding by its mark, so the difference survives a theme
            // that tints them the same.
            attr:data-group = (!runs).then(|| "true".to_owned())
        ) {
            label(class = "whichkey__key") {{keys}}
            label(class = "whichkey__arrow") {{if runs { "\u{2192}" } else { "\u{2192}+" }}}
            label(class = "whichkey__label nowrap") {{label}}
        }
    }
}
