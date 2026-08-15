//! What is open, and where it is shown.
//!
//! One value, held in the application's root context, that every part of the interface reads and
//! every action writes. The buffer line reads the order, the panes read the layout, the status
//! line reads the focused window — and an action like "open this file" is one method here rather
//! than a conversation between components.
//!
//! # What is reactive and what is not
//!
//! The *set* of buffers and the *arrangement* of windows are signals, because adding a buffer or
//! splitting a window changes what is on screen. What a buffer is at any moment — its revision,
//! whether it is saved — is a signal of its own inside the buffer, so that typing wakes one dirty
//! mark and not the whole buffer line.

mod buffer;
mod layout;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use rustc_hash::FxHashMap;
use slotmap::SlotMap;
use zdt_core::Project;
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

pub use crate::workspace::buffer::{Buffer, BufferId, BufferKind};
pub use crate::workspace::layout::{Axis, Direction, Layout, WindowId};

/// How many editors one window keeps mounted for buffers it is not showing.
///
/// A mounted editor holds its scroll position, its selections and its highlighter, so switching
/// back to a buffer this window has already visited is instant. Each one also holds a worker
/// thread, which is why the number is small — past it, the least recently seen is dropped and
/// costs a re-parse when it comes back.
const MOUNTED_PER_WINDOW: usize = 8;

/// One window: which buffer it shows, and which it is keeping ready.
#[derive(Clone)]
pub struct WindowState {
    /// Which buffer is on screen, when one is.
    ///
    /// A window with nothing in it is a real state rather than something to be avoided: closing
    /// the last buffer used to conjure an empty scratch one, which is a file nobody asked for
    /// sitting on the buffer line. An empty window says it is empty.
    pub current: Option<BufferId>,
    /// Which buffers have an editor mounted here, most recently seen first.
    pub mounted: Vec<BufferId>,
    /// How much larger or smaller this window's text is than the setting, in steps of one pixel.
    ///
    /// Per window rather than per application: `<C-+>` in a split is a request to read *this*
    /// file more comfortably, and shrinking the status line along with it is not what was asked.
    pub font_step: i32,
}

/// Everything that is open.
#[derive(Clone)]
pub struct Workspace {
    inner: Rc<Inner>,
}

struct Inner {
    project: Project,
    buffers: RwSignal<SlotMap<BufferId, Buffer>, LocalStorage>,
    /// The buffer line's order, which is the order buffers were opened in and `>b` reorders.
    order: RwSignal<Vec<BufferId>, LocalStorage>,
    windows: RwSignal<SlotMap<WindowId, WindowState>, LocalStorage>,
    layout: RwSignal<Layout, LocalStorage>,
    focused: RwSignal<WindowId, LocalStorage>,
    /// The buffer shown before the current one, which `<Leader>bp` goes back to.
    alternate: RwSignal<Option<BufferId>, LocalStorage>,
    /// What the interface is telling the user, shown in the status line until something replaces
    /// it.
    message: RwSignal<Option<Message>, LocalStorage>,
    /// The editor driving each mounted view, by the window and buffer it belongs to.
    ///
    /// Not a signal: nothing on screen is decided by which handles exist, and an action that
    /// needs one needs it right now rather than on the next flush.
    handles: RefCell<FxHashMap<(WindowId, BufferId), zgui_editor::EditorHandle>>,
    /// A number that changes whenever an editor arrives or goes away.
    ///
    /// The map itself is not a signal — nothing on screen is decided by which handles exist — but
    /// *when* one arrives matters to one thing: the effect that gives the keyboard to the focused
    /// window. A window made by `:split` is focused before its editor has mounted, so that effect
    /// runs, finds no editor and gives up. Without something to wake it the keyboard stays where
    /// it was, and `<C-j>` appears to do nothing until a later split happens to re-run it.
    mounted: RwSignal<u64, LocalStorage>,
    /// Every file opened this session, most recent first. Not a signal, for the same reason as
    /// the handles: it is read when a picker asks and never drawn.
    recent: RefCell<Vec<PathBuf>>,
    /// The reactive owner every buffer's signals are created under.
    ///
    /// A signal belongs to whichever owner was current when it was made, and dies with it. A
    /// buffer, though, is made from wherever the action that opened it was running — and that is
    /// a key handler on some element, whose owner is the pane it is mounted in. `<Leader>th`
    /// splits the window and *then* starts a terminal, so the buffer's signals were being created
    /// under a pane that the split had just taken apart: dead the moment they were made, and the
    /// buffer line panicked the first time it asked one of them for the tab's name.
    ///
    /// This is the workspace's own owner, taken once where the workspace itself is made. A buffer
    /// outlives every view of it by construction, so its signals have to as well.
    owner: zgui::reactive::Owner,
}

/// Something the interface is saying.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Message {
    /// The words.
    pub text: String,
    /// Whether it is a complaint.
    pub error: bool,
}

impl Workspace {
    /// An empty workspace on `project`, with one window showing one scratch buffer.
    ///
    /// There is always a window and always a buffer in it: an editor with nothing on screen has
    /// nowhere to put the caret, and every action would have to say what to do about it.
    pub fn new(project: Project) -> Self {
        let mut buffers = SlotMap::with_key();
        let scratch =
            buffers.insert_with_key(|id| Buffer::text(id, None, zgui_editor::Document::new("")));

        let mut windows = SlotMap::with_key();
        let window = windows.insert(WindowState {
            current: Some(scratch),
            mounted: vec![scratch],
            font_step: 0,
        });

        Self {
            inner: Rc::new(Inner {
                project,
                buffers: RwSignal::new_local(buffers),
                order: RwSignal::new_local(vec![scratch]),
                windows: RwSignal::new_local(windows),
                layout: RwSignal::new_local(Layout::Leaf(window)),
                focused: RwSignal::new_local(window),
                alternate: RwSignal::new_local(None),
                message: RwSignal::new_local(None),
                handles: RefCell::new(FxHashMap::default()),
                mounted: RwSignal::new_local(0),
                recent: RefCell::new(Vec::new()),
                owner: zgui::reactive::Owner::current().unwrap_or_default(),
            }),
        }
    }

    /// Makes something under the owner every buffer's signals belong to.
    ///
    /// Every buffer is created through this, and none is created any other way — see
    /// [`Inner::owner`].
    fn owned<T>(&self, make: impl FnOnce() -> T) -> T {
        self.inner.owner.with(make)
    }

    /// The directory everything is relative to.
    pub fn project(&self) -> &Project {
        &self.inner.project
    }

    // ---- Reading ---------------------------------------------------------------------------

    /// The buffer line's order. Tracked.
    pub fn order(&self) -> Vec<BufferId> {
        self.inner.order.get()
    }

    /// The arrangement of windows. Tracked.
    pub fn layout(&self) -> Layout {
        self.inner.layout.get()
    }

    /// Which window has the keyboard. Tracked.
    pub fn focused(&self) -> WindowId {
        self.inner.focused.get()
    }

    /// Which window has the keyboard, without subscribing.
    pub fn focused_untracked(&self) -> WindowId {
        self.inner.focused.get_untracked()
    }

    /// What the interface is saying. Tracked.
    pub fn message(&self) -> Option<Message> {
        self.inner.message.get()
    }

    /// Reads one buffer, when it is still open. Tracked.
    pub fn buffer(&self, id: BufferId) -> Option<Buffer> {
        self.inner.buffers.with(|buffers| buffers.get(id).cloned())
    }

    /// Reads one buffer without subscribing.
    pub fn buffer_untracked(&self, id: BufferId) -> Option<Buffer> {
        self.inner
            .buffers
            .with_untracked(|buffers| buffers.get(id).cloned())
    }

    /// Reads one window. Tracked.
    pub fn window(&self, id: WindowId) -> Option<WindowState> {
        self.inner.windows.with(|windows| windows.get(id).cloned())
    }

    /// The buffer the focused window is showing. Tracked.
    pub fn current_buffer(&self) -> Option<Buffer> {
        let window = self.window(self.focused())?;
        self.buffer(window.current?)
    }

    /// The buffer `id` shows, without subscribing.
    pub fn buffer_in_untracked(&self, window: WindowId) -> Option<BufferId> {
        self.inner
            .windows
            .with_untracked(|windows| windows.get(window).and_then(|state| state.current))
    }

    /// The buffer at `path`, when it is already open.
    pub fn find_path(&self, path: &Path) -> Option<BufferId> {
        self.inner.buffers.with_untracked(|buffers| {
            buffers
                .iter()
                .find(|(_, buffer)| buffer.is_at(path))
                .map(|(id, _)| id)
        })
    }

    // ---- Buffers ---------------------------------------------------------------------------

    /// Adds a buffer over `document` and shows it in the focused window.
    ///
    /// A file already open is shown rather than opened twice, which is what makes `<Leader>ff`
    /// onto something already on the buffer line a jump instead of a second copy of it.
    pub fn open_document(
        &self,
        path: Option<PathBuf>,
        document: zgui_editor::Document,
    ) -> BufferId {
        if let Some(path) = path.as_deref()
            && let Some(existing) = self.find_path(path)
        {
            self.show(existing);
            return existing;
        }

        let id = self
            .owned(|| {
                self.inner.buffers.try_update(|buffers| {
                    buffers.insert_with_key(|id| Buffer::text(id, path, document))
                })
            })
            .expect("the buffer map is writable");
        self.inner.order.update(|order| order.push(id));
        self.show(id);
        id
    }

    /// Adds a buffer that is not text, such as a terminal, and shows it.
    pub fn open_buffer(&self, make: impl FnOnce(BufferId) -> Buffer) -> BufferId {
        let id = self
            .owned(|| {
                self.inner
                    .buffers
                    .try_update(|buffers| buffers.insert_with_key(make))
            })
            .expect("the buffer map is writable");
        self.inner.order.update(|order| order.push(id));
        self.show(id);
        id
    }

    /// Puts `buffer` back in place of the one with its identity.
    ///
    /// For the fields that are read once the file has arrived — how it is spelled, what it breaks
    /// its lines with — which the buffer could not know when it was made.
    pub fn replace_buffer(&self, buffer: Buffer) {
        let id = buffer.id;
        self.inner.buffers.update(|buffers| {
            if let Some(held) = buffers.get_mut(id) {
                *held = buffer;
            }
        });
    }

    /// Shows `id` in the focused window.
    pub fn show(&self, id: BufferId) {
        self.show_in(self.focused_untracked(), id);
    }

    /// Shows `id` in `window`.
    ///
    /// The buffer it was showing becomes the alternate, which is what `<Leader>bp` goes back to,
    /// and stays mounted so that going back is instant.
    pub fn show_in(&self, window: WindowId, id: BufferId) {
        let previous = self.buffer_in_untracked(window);
        if previous == Some(id) {
            return;
        }
        if let Some(previous) = previous {
            self.inner.alternate.set(Some(previous));
        }
        // Every file shown is a file recently opened, whichever way it was reached — the picker,
        // the tree, the buffer line or the command line.
        if let Some(path) = self.buffer_untracked(id).and_then(|buffer| buffer.path) {
            self.remember(&path);
        }
        self.inner.windows.update(|windows| {
            let Some(state) = windows.get_mut(window) else {
                return;
            };
            state.current = Some(id);
            state.mounted.retain(|held| *held != id);
            state.mounted.insert(0, id);
            state.mounted.truncate(MOUNTED_PER_WINDOW);
        });
    }

    /// Opens a terminal buffer called `name`, and answers it.
    ///
    /// The program itself is [`crate::terminals`]'s business; what this makes is the buffer it
    /// will be drawn in, so that a terminal is on the buffer line like everything else.
    /// A floating terminal is `listed = false`: it is reachable by the key that toggles it and by
    /// nothing else, which is what makes it a scratch terminal rather than another tab to close.
    pub fn open_terminal(&self, name: &str, listed: bool) -> BufferId {
        let id = self
            .owned(|| {
                self.inner
                    .buffers
                    .try_update(|buffers| buffers.insert_with_key(|id| Buffer::terminal(id, name)))
            })
            .expect("the buffer map is writable");
        if listed {
            self.inner.order.update(|order| order.push(id));
            self.show(id);
        }
        id
    }

    /// Puts the title a program asked for on its buffer.
    pub fn rename_terminal(&self, id: BufferId, title: Option<String>) {
        let Some(buffer) = self.buffer_untracked(id) else {
            return;
        };
        let crate::workspace::BufferKind::Terminal { title: held } = &buffer.kind else {
            return;
        };
        // A program that clears its title leaves the one it had: an empty tab says less than a
        // stale one.
        if let Some(title) = title.filter(|title| !title.is_empty())
            && held.get_untracked().as_deref() != Some(title.as_str())
        {
            held.set(Some(title));
        }
    }

    /// Closes `id`, showing something else wherever it was.
    ///
    /// The buffer's text goes with it: a closed buffer is closed, and its undo history is not
    /// something an editor keeps for a file nobody has open. Answers whether there was one.
    pub fn close_buffer(&self, id: BufferId) -> bool {
        // What a window showing this should show instead: the next buffer along, or nothing when
        // that was the last one. Nothing is a real answer — an empty window says so.
        let order = self.inner.order.get_untracked();
        let at = order.iter().position(|held| *held == id);
        let replacement = at.and_then(|at| {
            order
                .get(at + 1)
                .or_else(|| at.checked_sub(1).and_then(|before| order.get(before)))
                .copied()
        });

        let existed = self
            .inner
            .buffers
            .try_update(|buffers| buffers.remove(id).is_some())
            .unwrap_or(false);
        if !existed {
            return false;
        }

        self.inner
            .order
            .update(|order| order.retain(|held| *held != id));
        self.inner.alternate.update(|alternate| {
            if *alternate == Some(id) {
                *alternate = None;
            }
        });
        self.inner.windows.update(|windows| {
            for state in windows.values_mut() {
                state.mounted.retain(|held| *held != id);
                if state.current == Some(id) {
                    state.current = replacement;
                    if let Some(replacement) = replacement
                        && !state.mounted.contains(&replacement)
                    {
                        state.mounted.insert(0, replacement);
                    }
                }
            }
        });
        true
    }

    /// Shows the buffer `offset` places along the buffer line from the current one, wrapping.
    pub fn cycle_buffer(&self, offset: isize) {
        let order = self.inner.order.get_untracked();
        if order.len() < 2 {
            return;
        }
        let Some(current) = self.buffer_in_untracked(self.focused_untracked()) else {
            return;
        };
        let Some(index) = order.iter().position(|held| *held == current) else {
            return;
        };
        let count = order.len() as isize;
        let next = (index as isize + offset).rem_euclid(count) as usize;
        self.show(order[next]);
    }

    /// Goes back to the buffer that was shown before this one.
    pub fn show_alternate(&self) {
        if let Some(alternate) = self.inner.alternate.get_untracked() {
            self.show(alternate);
        }
    }

    /// Moves the current buffer `offset` places along the buffer line.
    pub fn move_buffer(&self, offset: isize) {
        let Some(current) = self.buffer_in_untracked(self.focused_untracked()) else {
            return;
        };
        self.inner.order.update(|order| {
            let Some(index) = order.iter().position(|held| *held == current) else {
                return;
            };
            let count = order.len() as isize;
            let target = (index as isize + offset).clamp(0, count - 1) as usize;
            let id = order.remove(index);
            order.insert(target, id);
        });
    }

    // ---- Windows ---------------------------------------------------------------------------

    /// Splits the focused window along `axis`, showing the same buffer in both.
    pub fn split(&self, axis: Axis) -> Option<WindowId> {
        let focused = self.focused_untracked();
        let current = self.buffer_in_untracked(focused)?;
        let new = self
            .inner
            .windows
            .try_update(|windows| {
                windows.insert(WindowState {
                    current: Some(current),
                    mounted: vec![current],
                    font_step: 0,
                })
            })
            .expect("the window map is writable");
        let split = self
            .inner
            .layout
            .try_update(|layout| layout.split(focused, axis, new))
            .unwrap_or(false);
        if !split {
            self.inner.windows.update(|windows| {
                windows.remove(new);
            });
            return None;
        }
        self.inner.focused.set(new);
        Some(new)
    }

    /// Closes the focused window, unless it is the only one.
    pub fn close_window(&self) -> bool {
        let focused = self.focused_untracked();
        let closed = self
            .inner
            .layout
            .try_update(|layout| layout.close(focused))
            .unwrap_or(false);
        if !closed {
            return false;
        }
        self.inner.windows.update(|windows| {
            windows.remove(focused);
        });
        if let Some(next) = self.inner.layout.get_untracked().windows().first().copied() {
            self.inner.focused.set(next);
        }
        true
    }

    /// Gives the keyboard to `window`.
    pub fn focus_window(&self, window: WindowId) {
        if self.inner.focused.get_untracked() != window {
            self.inner.focused.set(window);
        }
    }

    /// Gives the keyboard to the next window in the walking order.
    pub fn cycle_window(&self, forward: bool) {
        let layout = self.inner.layout.get_untracked();
        let focused = self.inner.focused.get_untracked();
        let next = if forward {
            layout.next_after(focused)
        } else {
            layout.previous_before(focused)
        };
        if let Some(next) = next {
            self.inner.focused.set(next);
        }
    }

    /// Writes the sizes a dragged handle reported into the split it belongs to.
    pub fn resize(&self, window: WindowId, sizes: &[f64]) {
        self.inner.layout.update(|layout| {
            layout.resize(window, sizes);
        });
    }

    // ---- The editors themselves -------------------------------------------------------------

    /// Remembers the editor showing `buffer` in `window`.
    pub fn register_handle(
        &self,
        window: WindowId,
        buffer: BufferId,
        handle: zgui_editor::EditorHandle,
    ) {
        self.inner
            .handles
            .borrow_mut()
            .insert((window, buffer), handle);
        self.inner
            .mounted
            .update(|revision| *revision = revision.wrapping_add(1));
    }

    /// A number that changes whenever an editor arrives or goes away. Tracked.
    ///
    /// What the focus effect reads so that an editor mounting after its window was focused still
    /// ends up with the keyboard.
    #[must_use]
    pub fn mounted_revision(&self) -> u64 {
        self.inner.mounted.get()
    }

    /// Forgets it, which a view does as it unmounts.
    ///
    /// `handle` is the editor that is going away, and the entry is dropped only if it is still the
    /// one filed here. A pane rebuilt in place — which is what splitting does to every pane the
    /// new layout re-creates — mounts its new editor *before* the old one is cleaned up, so the
    /// two orders overlap: register the new, then forget the old. Forgetting by key alone deletes
    /// the registration the new editor had just made, and the window is then a window with an
    /// editor on the screen and no handle to it. Nothing draws differently; what breaks is
    /// everything that asks the workspace for the editor of a window, `<C-k>` first among them.
    pub fn forget_handle(
        &self,
        window: WindowId,
        buffer: BufferId,
        handle: &zgui_editor::EditorHandle,
    ) {
        let mut handles = self.inner.handles.borrow_mut();
        if handles.get(&(window, buffer)) != Some(handle) {
            return;
        }
        handles.remove(&(window, buffer));
        drop(handles);
        self.inner
            .mounted
            .update(|revision| *revision = revision.wrapping_add(1));
    }

    /// The editor showing `buffer` in `window`, when one is mounted.
    pub fn handle_for(
        &self,
        window: WindowId,
        buffer: BufferId,
    ) -> Option<zgui_editor::EditorHandle> {
        self.inner.handles.borrow().get(&(window, buffer)).cloned()
    }

    /// The editor the keyboard is in, when it is in one.
    ///
    /// What every action that edits text goes through, and the one place that answers "the
    /// editor" — everything else says which window it means.
    pub fn current_handle(&self) -> Option<zgui_editor::EditorHandle> {
        let window = self.focused_untracked();
        let buffer = self.buffer_in_untracked(window)?;
        self.handle_for(window, buffer)
    }

    /// Every file that has been open in this session, the most recent first.
    ///
    /// Kept rather than derived from the open buffers, because the point of a recent-files list is
    /// the ones that are *not* open any more.
    #[must_use]
    pub fn recent(&self) -> Vec<PathBuf> {
        self.inner.recent.borrow().clone()
    }

    /// Remembers `path` as the most recently opened.
    pub fn remember(&self, path: &Path) {
        let mut recent = self.inner.recent.borrow_mut();
        recent.retain(|held| held != path);
        recent.insert(0, path.to_path_buf());
        // A session's worth, which is as much as anybody scrolls: this is not a history file.
        recent.truncate(200);
    }

    /// Moves the keyboard to the window `direction` of the focused one.
    ///
    /// Answers whether there was one. Nothing that way is not an error: `<C-w>h` in the leftmost
    /// window is a key that does nothing, the same as in vim.
    /// Makes this window's text `step` pixels larger, or puts it back when `step` is zero.
    pub fn zoom(&self, window: WindowId, step: i32) {
        self.inner.windows.update(|windows| {
            let Some(state) = windows.get_mut(window) else {
                return;
            };
            state.font_step = if step == 0 {
                0
            } else {
                // Bounded either way: text of no pixels draws nothing, and text the size of the
                // window leaves no room for any.
                (state.font_step + step).clamp(-6, 24)
            };
        });
    }

    /// How much larger this window's text is than the setting. Tracked.
    #[must_use]
    pub fn font_step(&self, window: WindowId) -> i32 {
        self.inner
            .windows
            .with(|windows| windows.get(window).map_or(0, |state| state.font_step))
    }

    pub fn focus_direction(&self, direction: crate::workspace::Direction) -> bool {
        let from = self.focused_untracked();
        let Some(next) = self
            .inner
            .layout
            .with_untracked(|layout| layout.neighbour(from, direction))
        else {
            return false;
        };
        self.focus_window(next);
        true
    }

    /// Gives the keyboard back to the editor, wherever it went.
    pub fn focus_editor(&self) {
        if let Some(handle) = self.current_handle() {
            handle.focus();
        }
    }

    // ---- Saying things ----------------------------------------------------------------------

    /// Says something in the status line.
    pub fn say(&self, text: impl Into<String>) {
        self.inner.message.set(Some(Message {
            text: text.into(),
            error: false,
        }));
    }

    /// Complains in the status line.
    pub fn complain(&self, text: impl Into<String>) {
        let text = text.into();
        tracing::warn!("{text}");
        self.inner.message.set(Some(Message { text, error: true }));
    }

    /// Takes back whatever was being said.
    pub fn hush(&self) {
        if self.inner.message.get_untracked().is_some() {
            self.inner.message.set(None);
        }
    }
}

/// Puts the workspace where every component can find it.
pub fn provide(workspace: Workspace) {
    zgui::reactive::provide_local_context(workspace);
}

/// The workspace, from inside a component.
///
/// # Panics
///
/// If no workspace was provided above this component, which is a wiring mistake rather than a
/// state anything can carry on from.
pub fn use_workspace() -> Workspace {
    zgui::reactive::use_local_context::<Workspace>()
        .expect("a workspace is provided at the application root")
}
