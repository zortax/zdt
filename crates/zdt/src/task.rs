//! Work that has to finish.
//!
//! An ordinary `spawn_local` belongs to whatever component was being built when it was called, and
//! is cancelled when that component goes away. That is the right rule for anything drawing — a
//! request whose answer nobody will see is a request worth dropping.
//!
//! It is the wrong rule for the filesystem. A key pressed in a panel often *is* what closes that
//! panel: submitting the tree's rename prompt disposes the prompt, and the rename spawned from
//! inside it would be cancelled before it ever ran. The same holds for a picker that opens a file
//! as it closes, and for a save on the way out.
//!
//! So work with an effect outside the interface is started here instead, where nothing cancels it.
//! What it writes back is a signal, and writing a signal whose window has gone is harmless.

/// Runs `work` to completion, whatever happens to the component that started it.
///
/// For anything that touches the filesystem or another process. Reactive work — a query whose
/// answer is only ever drawn — should use [`zgui::task::spawn_local`] instead, so that closing the
/// thing that asked also stops the asking.
///
/// # Panics
///
/// In debug builds, if called off the interface thread.
pub fn detached(work: impl Future<Output = ()> + 'static) {
    zgui::task::spawn_detached(work);
}
