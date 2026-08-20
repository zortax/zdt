//! The two things a region says about itself.
//!
//! Both are registered once, where the region is built, and both are released by the scope going
//! away. There is nothing to hold and nothing to give back, which is what makes "forgot to hand the
//! keyboard back" a state that cannot be reached.
//!
//! # The rule the releases obey
//!
//! A release writes and never reads. It runs while a scope is being disposed of, where that scope's
//! own signals have gone, and a panic there aborts rather than unwinds. Every write here is
//! fallible for the same reason.

use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RenderEffect, Signal, on_cleanup_local};

use super::{Overlay, Sink, Spot, try_use_focus};

/// Says that `overlay` has the keys while `present` is true.
///
/// `present` is the signal the overlay already draws itself from, so this is one line beside an
/// expression that is there anyway. The claim goes when the overlay closes, and the scope going
/// away is the backstop.
pub fn claim(overlay: Overlay, present: Signal<bool, LocalStorage>) {
    claim_named(Signal::derive_local(move || {
        present.get().then_some(overlay)
    }));
}

/// The same, for an overlay that says which one it is.
///
/// The floating terminal names the program it is showing, so swapping one float for another is one
/// claim released and another taken rather than a claim that quietly changed meaning.
pub fn claim_named(wanted: Signal<Option<Overlay>, LocalStorage>) {
    // Looked up here, while this is being built. A context asked for inside the effect below would
    // be asked for from a timer's scope, where the answer is nothing.
    let Some(focus) = try_use_focus() else {
        return;
    };

    let following = {
        let focus = focus.clone();
        RenderEffect::new(move |previous: Option<Option<Overlay>>| {
            let now = wanted.get();
            if let Some(Some(before)) = previous
                && Some(before) != now
            {
                focus.pop(before);
            }
            if let Some(overlay) = now {
                focus.push(overlay);
            }
            now
        })
    };

    on_cleanup_local(move || {
        // The effect first, so nothing can put the claim back after it is released.
        let last = following.take_value().flatten();
        drop(following);
        if let Some(overlay) = last {
            focus.pop(overlay);
        }
    });
}

/// Says how `spot` takes the keyboard, for as long as this scope is alive.
///
/// A spot with no sink is a spot the keyboard never moves to, which is what the layers that take
/// only the keys want: documentation, suggestions, tab labels and a leap all leave the caret where
/// it was.
pub fn sink(spot: Spot, sink: Sink) {
    let Some(focus) = try_use_focus() else {
        return;
    };
    focus.register(spot, sink);
    on_cleanup_local(move || focus.forget(spot));
}
