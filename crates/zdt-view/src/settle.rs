//! A derivation that answers only when its value moves.
//!
//! A closure over signals re-runs its readers whenever any input notifies, whether or not the
//! answer changed. That is the right rule for drawing, and the wrong one for a value many
//! readers hang off: a streamed word notifies a whole row, and everything derived from the row
//! recomputes to arrive at the same answer. `settled` puts a comparison between the two — the
//! derivation re-runs with its inputs, and its readers run only when the answer is new.
//!
//! The reactive library's own memo asks its closure to be `Send`, and a view's data lives on the
//! interface thread behind `Rc`. This is the local form.

use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RenderEffect, RwSignal, on_cleanup_local};

/// `compute`, deduplicated: a signal that follows it and notifies only on change.
///
/// The effect that follows lives with the scope this is called in, and so does the signal: a
/// signal made inside an effect would belong to one run of it and be gone by the next.
pub fn settled<T>(compute: impl Fn() -> T + 'static) -> RwSignal<T, LocalStorage>
where
    T: PartialEq + 'static,
{
    // Computed once here for the starting value, and again by the effect's first run, which is
    // the run that subscribes the computation to its inputs.
    let value: RwSignal<T, LocalStorage> = RwSignal::new_local(compute());
    let following = RenderEffect::new(move |_| {
        let next = compute();
        if value.with_untracked(|held| *held != next) {
            value.set(next);
        }
    });
    on_cleanup_local(move || drop(following));
    value
}
