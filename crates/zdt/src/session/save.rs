//! Writing a session down, and deciding when.
//!
//! # Three debounces
//!
//! The three things that change cost very different amounts and happen at very different rates,
//! so each waits its own while:
//!
//! | what moved | waits | why |
//! |---|---|---|
//! | a split, the buffer line, the tree | 2 s | a few kilobytes, and rare |
//! | the text | 5 s | a whole undo history, and one burst of typing is one write |
//! | where a view is looking | 20 s | one scroll is dozens of events, and a lost scroll costs a keypress |
//!
//! Each is debounced on the session's own [`Clock`](zdt_view::Clock), which a window lends its
//! engine to. A session nobody is looking at therefore does not write — and does not need to,
//! because nothing in it is changing.
//!
//! # Nothing touches a file on the interface thread
//!
//! Only the interface thread can read a `Document` or a signal, and only a worker may block. So a
//! save is in two halves: [`Writer::owed`] runs where the state is and reads *everything* it
//! needs, and [`perform`] runs on a worker and is the only half that knows about files. What
//! crosses between them is plain data.
//!
//! That is why the last-written manifest is kept in memory rather than read back before each
//! save: deciding which blobs can be reused is the interface thread's half of the work, and it
//! must not mean reading a file to find out.
//!
//! Quitting is the one exception, and deliberately: the process is about to stop, so the last
//! save is done in place rather than handed to a worker that will not be polled again.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use rustc_hash::FxHashSet;
use zgui::prelude::*;

use crate::session::capture::{self, Views};
use crate::session::schema::{BufferContent, BufferSort, FORMAT, HistorySnapshot, Snapshot};
use crate::session::store;
use crate::session::{Session, SessionKey};
use crate::workspace::BufferId;

/// How long after a split moves the manifest is written.
const STRUCTURE: Duration = Duration::from_secs(2);

/// How long after the last keystroke a buffer's text is written.
const TEXT: Duration = Duration::from_secs(5);

/// How long after a view stops moving its place is written.
///
/// The longest of the three, because scrolling is the most frequent thing an editor does and the
/// least expensive to lose: a scroll that does not reach the disk costs one keypress to redo,
/// where a lost edit costs the edit.
const VIEW: Duration = Duration::from_secs(20);

/// The most of one buffer's undo history that is worth keeping.
const HISTORY_STEPS: usize = 500;

/// And the most text that history may hold, in bytes.
const HISTORY_BYTES: usize = 4 << 20;

/// The largest unsaved buffer that is written at all.
///
/// Above this the text is left out and the buffer reopens from disk. A session must not become
/// the place somebody's editor keeps a copy of a very large file.
const UNSAVED_BYTES: usize = 32 << 20;

/// One save, as the worker receives it.
///
/// Everything only the interface thread could read is already in here. Nothing in it borrows, so
/// it crosses to a worker whole.
struct Owed {
    directory: PathBuf,
    snapshot: Snapshot,
    /// The buffers whose blob is being rewritten, by their place in the snapshot.
    blobs: Vec<(usize, BufferContent)>,
    /// The buffers whose file has to be looked at, by their place in the snapshot.
    ///
    /// Hashing a file means reading all of it, which is exactly the work that must not happen on
    /// the interface thread. Only the buffers being rewritten are in here; the rest keep the
    /// stamp they were written with.
    stamping: Vec<(usize, PathBuf)>,
}

/// What is owed, and what has already been written.
///
/// One per session, held for its life.
pub struct Writer {
    session: Session,
    /// Counts up once per write, so a second editor writing the same session can be noticed.
    generation: Cell<u64>,
    /// Where each editor was looking, including editors that have gone away.
    views: Rc<RefCell<Views>>,
    /// Which buffers' text has moved since the last write.
    stale: RefCell<FxHashSet<BufferId>>,
    /// What was last written, so a blob that is still good is kept without reading it back.
    written: RefCell<Option<Snapshot>>,
    /// Whether a worker is part-way through a save. A second one waits rather than interleaving.
    writing: Cell<bool>,
    /// What is waiting to be written.
    structure: RefCell<Option<zdt_view::Pending>>,
    text: RefCell<Option<zdt_view::Pending>>,
    view: RefCell<Option<zdt_view::Pending>>,
}

impl Writer {
    /// A writer for `session`, at `generation`, holding what it was restored from.
    #[must_use]
    pub fn new(
        session: Session,
        generation: u64,
        views: Rc<RefCell<Views>>,
        written: Option<Snapshot>,
    ) -> Rc<Self> {
        Rc::new(Self {
            session,
            generation: Cell::new(generation),
            views,
            stale: RefCell::new(FxHashSet::default()),
            written: RefCell::new(written),
            writing: Cell::new(false),
            structure: RefCell::new(None),
            text: RefCell::new(None),
            view: RefCell::new(None),
        })
    }

    /// Says something structural moved: a split, the buffer list, the tree.
    pub fn touched(self: &Rc<Self>) {
        let writer = Rc::clone(self);
        let owed = self
            .session
            .clock()
            .after(STRUCTURE, move || writer.write_soon(false));
        // Replacing the handle cancels the one before it, which is the debounce.
        *self.structure.borrow_mut() = Some(owed);
    }

    /// Says one buffer's text moved.
    pub fn touched_text(self: &Rc<Self>, buffer: BufferId) {
        self.stale.borrow_mut().insert(buffer);
        let writer = Rc::clone(self);
        let owed = self
            .session
            .clock()
            .after(TEXT, move || writer.write_soon(true));
        *self.text.borrow_mut() = Some(owed);
    }

    /// Remembers where an editor is looking, and says so is worth writing down before long.
    ///
    /// Called from the editor's own move events, so this runs many times a second while somebody
    /// scrolls. Everything it does is in memory; the write it arms is the slowest of the three.
    pub fn remember_view(
        self: &Rc<Self>,
        window: crate::workspace::WindowId,
        buffer: BufferId,
        handle: &zgui_editor::EditorHandle,
    ) {
        // Asked first, stored second. A method call evaluates its receiver before its arguments,
        // so borrowing and *then* asking the editor would hold this open across a call that can
        // come back here.
        let view = capture::look(handle);
        let moved = self
            .views
            .borrow_mut()
            .insert((window, buffer), view.clone())
            .is_none_or(|held| held != view);
        if !moved {
            return;
        }
        let writer = Rc::clone(self);
        let owed = self
            .session
            .clock()
            .after(VIEW, move || writer.write_soon(false));
        *self.view.borrow_mut() = Some(owed);
    }

    /// Writes whatever is owed, now and in place.
    ///
    /// What closing a window and quitting both do. The one save that blocks the interface thread,
    /// because the alternative is handing it to a worker nothing will poll again.
    pub fn flush(self: &Rc<Self>) {
        // Every one taken, and not just until the first: all three have to be disarmed, or a
        // timer left armed would fire against a session that has gone.
        let owed = [&self.structure, &self.text, &self.view]
            .into_iter()
            .filter(|held| held.borrow_mut().take().is_some())
            .count()
            > 0;
        if !owed {
            return;
        }
        let Some(owed) = self.owed(true) else {
            return;
        };
        *self.written.borrow_mut() = Some(perform(owed));
    }

    /// Reads everything only this thread can, and hands the rest to a worker.
    fn write_soon(self: &Rc<Self>, with_text: bool) {
        // A save already on its way. Waiting is right: the one in flight is about to write state
        // that is at most a moment old, and this one would only race it to the same file.
        if self.writing.get() {
            if with_text {
                self.touched_text_again();
            } else {
                self.touched();
            }
            return;
        }
        let Some(owed) = self.owed(with_text) else {
            return;
        };

        self.writing.set(true);
        let writer = Rc::clone(self);
        zdt_view::detached(async move {
            let written = zgui::task::blocking(move || perform(owed)).await;
            *writer.written.borrow_mut() = Some(written);
            writer.writing.set(false);
        });
    }

    /// Arms the text debounce again without marking anything else stale.
    fn touched_text_again(self: &Rc<Self>) {
        let writer = Rc::clone(self);
        let owed = self
            .session
            .clock()
            .after(TEXT, move || writer.write_soon(true));
        *self.text.borrow_mut() = Some(owed);
    }

    /// Everything a save needs, read from where it lives.
    ///
    /// On the interface thread, and touching no file: the documents, the signals and the last
    /// manifest are all in memory here.
    fn owed(self: &Rc<Self>, with_text: bool) -> Option<Owed> {
        let state = self.session.state()?;
        let root = self.session.project().root().to_path_buf();
        let directory = store::directory_for(&state, &root);

        self.generation.set(self.generation.get() + 1);
        // Read and let go before anything else: taking a snapshot reaches into the workspace, and
        // the workspace can reach back here through a view recording where it is looking.
        let views = self.views.borrow().clone();
        let mut snapshot = capture::capture(&self.session, &views, self.generation.get());

        let previous = self.written.borrow();
        let stale = std::mem::take(&mut *self.stale.borrow_mut());
        let order = self.session.workspace().order_untracked();

        let mut blobs = Vec::new();
        let mut stamping = Vec::new();
        for (at, buffer) in snapshot.buffers.iter_mut().enumerate() {
            if buffer.kind != BufferSort::Text {
                continue;
            }
            let id = order.get(at).copied();
            let moved = with_text && id.is_some_and(|id| stale.contains(&id));
            let held = previous
                .as_ref()
                .and_then(|held| held.buffers.get(at))
                .filter(|held| held.path == buffer.path);

            if !moved && let Some(held) = held {
                // Nothing changed here: keep the blob that is already on disk, and the stamp it
                // was written with. Not looking at the file again is the point.
                buffer.content.clone_from(&held.content);
                buffer.disk = held.disk;
                continue;
            }

            let Some(id) = id else {
                continue;
            };
            let Some(content) = describe_content(&self.session, id) else {
                continue;
            };
            if let Some(path) = buffer.path.clone() {
                stamping.push((at, path));
            }
            blobs.push((at, content));
        }
        drop(previous);

        Some(Owed {
            directory,
            snapshot,
            blobs,
            stamping,
        })
    }
}

/// The half of a save that touches files. Blocking, and never called on the interface thread
/// except by [`Writer::flush`].
///
/// Answers the manifest as written, so the next save knows which blobs it may keep.
fn perform(mut owed: Owed) -> Snapshot {
    // What the files look like right now. Reading one to hash it is the most expensive thing here
    // and the reason none of this belongs on the interface thread.
    for (at, path) in &owed.stamping {
        if let Some(buffer) = owed.snapshot.buffers.get_mut(*at) {
            buffer.disk = Some(capture::stamp(path));
        }
    }

    for (at, content) in &owed.blobs {
        match store::write_blob(&owed.directory, *at, content) {
            Ok(reference) => {
                if let Some(buffer) = owed.snapshot.buffers.get_mut(*at) {
                    buffer.content = Some(reference);
                }
            }
            Err(error) => tracing::warn!("a buffer would not write: {error}"),
        }
    }

    // The manifest last, so a crash leaves orphan blobs rather than a manifest naming files that
    // are not there.
    if let Err(error) = store::write_manifest(&owed.directory, &owed.snapshot) {
        tracing::warn!("the session would not write: {error}");
        return owed.snapshot;
    }

    let keeping: Vec<String> = owed
        .snapshot
        .buffers
        .iter()
        .filter_map(|buffer| buffer.content.as_ref().map(|content| content.file.clone()))
        .collect();
    store::sweep_blobs(&owed.directory, &keeping);
    owed.snapshot
}

/// One buffer's text and history, bounded so that a session stays a session.
fn describe_content(session: &Session, buffer: BufferId) -> Option<BufferContent> {
    let entry = session.workspace().buffer_untracked(buffer)?;
    let document = entry.document()?;
    let dirty = entry.dirty.get_untracked();
    let text = document.text();

    // Too large to be worth keeping a copy of. It reopens from disk; the dirty text is lost, and
    // that is said at capture time rather than discovered at restore time.
    if text.len() > UNSAVED_BYTES {
        return Some(BufferContent {
            format: FORMAT,
            text: None,
            dirty: false,
            history: HistorySnapshot::default(),
            trimmed: true,
        });
    }

    let (history, trimmed) = document.with_history(|history| {
        let mut held = zgui_editor::History::from_parts(
            history.undo_steps().to_vec(),
            history.redo_steps().to_vec(),
        );
        let trimmed = held.trim(HISTORY_STEPS, HISTORY_BYTES);
        (
            HistorySnapshot {
                undo: held.undo_steps().to_vec(),
                redo: held.redo_steps().to_vec(),
            },
            trimmed,
        )
    });

    Some(BufferContent {
        format: FORMAT,
        // A clean buffer's text is already on disk, and writing it twice would double the size of
        // a session for nothing. Only what differs is kept.
        text: dirty.then_some(text),
        dirty,
        history,
        trimmed,
    })
}

/// Reads the session for `key`, when there is one.
///
/// Synchronous, and called once before the workspace is made: the first frame has to be the right
/// one, and one small file is what that costs.
#[must_use]
pub fn read_for(key: &SessionKey) -> Option<Snapshot> {
    let state = zdt_core::state::State::discover()?;
    let root = key.path()?;
    store::read(&state, root)
}

/// Removes sessions that are no longer worth keeping.
///
/// Once, at startup, on a worker. `keeping` is never pruned however old it is.
pub fn prune_soon(keeping: std::path::PathBuf) {
    let Some(state) = zdt_core::state::State::discover() else {
        return;
    };
    zdt_view::detached(async move {
        zgui::task::blocking(move || store::prune(&state, &keeping)).await;
    });
}

#[cfg(test)]
mod tests {
    use super::{HISTORY_BYTES, HISTORY_STEPS, STRUCTURE, TEXT, UNSAVED_BYTES, VIEW};

    #[test]
    fn what_costs_least_to_lose_waits_longest() {
        // Scrolling is the most frequent thing an editor does and the cheapest to redo; an edit
        // is neither.
        const _: () = assert!(STRUCTURE.as_secs() < TEXT.as_secs());
        const _: () = assert!(TEXT.as_secs() < VIEW.as_secs());
    }

    #[test]
    fn the_bounds_leave_room_for_ordinary_work() {
        // Five hundred undo steps is more than a day of editing one file, and four megabytes of
        // history is far more than that.
        const _: () = assert!(HISTORY_STEPS >= 100);
        const _: () = assert!(HISTORY_BYTES >= 1 << 20);
        const _: () = assert!(UNSAVED_BYTES > HISTORY_BYTES);
    }
}
