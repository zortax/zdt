//! Work that has to finish.
//!
//! An ordinary `spawn_local` belongs to whatever component was being built when it was called. It
//! is cancelled when that component goes away. That is the right rule for anything drawing: a
//! request whose answer nobody will see is a request to drop.
//!
//! It is the wrong rule for the filesystem. A key pressed in a panel often is what closes that
//! panel. Submitting the tree's rename prompt disposes the prompt, and the rename spawned from
//! inside it would be cancelled before it ever ran. The same holds for a picker that opens a file
//! as it closes, and for a save on the way out.
//!
//! So work with an effect outside the interface starts here, where nothing cancels it. What it
//! writes back is a signal, and writing a signal whose window has gone is harmless.

/// Runs `work` to completion, whatever happens to the component that started it.
///
/// For anything that touches the filesystem or another process. Reactive work, meaning a query
/// whose answer is only ever drawn, belongs in [`zgui::task::spawn_local`]. Closing the thing that
/// asked then stops the asking.
///
/// # Panics
///
/// In debug builds, if called off the interface thread.
pub fn detached(work: impl Future<Output = ()> + 'static) {
    zgui::task::spawn_detached(work);
}
