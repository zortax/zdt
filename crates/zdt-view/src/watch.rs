//! Noticing that a directory changed.
//!
//! A watcher on one directory, debounced, reporting on the interface thread. What it reports is
//! *that* something changed. What to do about it belongs to the caller.

use std::path::Path;
use std::time::Duration;

use zgui::task::spawn_local;
use zgui::tokio::spawn_receiver;

/// How long to wait after a change before reporting.
///
/// Every editor's atomic save writes several files. This makes them one read at the end of the
/// wait, and not four.
const SETTLE: Duration = Duration::from_millis(120);

/// Watches `directory` and calls `changed` on the interface thread whenever something under it
/// moves.
///
/// The watcher runs on its own thread and is kept alive by the returned handle. Dropping the
/// handle stops the watching. A directory that does not exist answers `None`: somebody who has
/// never configured anything has none, and may make one later.
#[must_use]
pub fn watch(directory: &Path, changed: impl Fn() + 'static) -> Option<Watcher> {
    use notify::{RecursiveMode, Watcher as _};

    // A tokio channel, because the receiving end is awaited on the interface thread and
    // `spawn_receiver` is what turns it into a call there. The sending end goes to the watcher's
    // own thread, so it has to be a channel that crosses threads.
    let (tx, rx) = tokio::sync::mpsc::channel::<()>(16);

    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        // Anything that is not a read: written, made, removed, renamed.
        let interesting = event
            .map(|event| !matches!(event.kind, notify::EventKind::Access(_)))
            .unwrap_or(false);
        if interesting {
            // Full is fine. A change is a change, and one that arrives while another is being
            // dealt with is the same reload.
            let _ = tx.try_send(());
        }
    })
    .ok()?;

    // The directory and not the files. An atomic save replaces a file, so a watch on the file
    // itself would follow the one that was renamed away.
    if watcher.watch(directory, RecursiveMode::Recursive).is_err() {
        return None;
    }

    // One task takes what arrives and reports it after a pause: an atomic save writes a temporary
    // and renames it, which is two events for one change. The report is shared and not moved,
    // because it is called once per change and the channel goes on delivering.
    let changed = std::rc::Rc::new(changed);
    // One flag per watch, and never one for the thread. Several watches run at once — the
    // configuration directory, a repository, a project — and a flag they shared would let one
    // watch's pause swallow another watch's change.
    let held = std::rc::Rc::new(std::cell::Cell::new(false));
    let pump = spawn_receiver(rx, move |()| {
        if held.replace(true) {
            // Something is already waiting to report. This change joins it.
            return;
        }
        let changed = std::rc::Rc::clone(&changed);
        let held = std::rc::Rc::clone(&held);
        let task = spawn_local(async move {
            zgui::task::blocking(move || std::thread::sleep(SETTLE)).await;
            held.set(false);
            changed();
        });
        // The task outlives this call by design. The watch's own handle is what ends the
        // reporting, because dropping it drops the channel and ends the pump.
        std::mem::forget(task);
    });

    Some(Watcher {
        _watcher: watcher,
        _pump: pump,
    })
}

/// Keeps a watch alive. Dropping it stops the watching.
pub struct Watcher {
    _watcher: notify::RecommendedWatcher,
    _pump: zgui::task::Task,
}
