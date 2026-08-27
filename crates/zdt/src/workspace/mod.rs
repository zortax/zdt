//! What is open, and where it is shown.
//!
//! One value, held in the application's root context, that every part of the interface reads and
//! every action writes. The buffer line reads the order, the panes read the layout, and the status
//! line reads the focused window. An action like "open this file" is one method here, and never a
//! conversation between components.
//!
//! # What is reactive and what is not
//!
//! The *set* of buffers and the *arrangement* of windows are signals, because adding a buffer or
//! splitting a window changes what is on screen. What a buffer is at any moment, meaning its
//! revision and whether it is saved, is a signal of its own inside the buffer. So typing wakes one
//! dirty mark and leaves the buffer line alone.

pub mod pane;
pub mod panes;

pub mod buffer;
mod buffers;
mod editors;
mod layout;
mod read;
mod restore;
mod say;
mod windows;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use rustc_hash::FxHashMap;
use slotmap::SlotMap;
use zdt_core::Project;
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

pub use crate::workspace::buffer::{Buffer, BufferId, BufferKind};
pub use crate::workspace::layout::{Axis, Direction, Layout, Shape, WindowId};
pub use crate::workspace::restore::Restored;

/// How many editors one window keeps mounted for buffers it is not showing.
///
/// A mounted editor holds its scroll position, its selections and its highlighter, so switching
/// back to a buffer this window has already visited is instant. Each one also holds a worker
/// thread, which is why the number is small. Past it, the least recently seen is dropped, and
/// costs a re-parse when it comes back.
const MOUNTED_PER_WINDOW: usize = 8;

/// One window: which buffer it shows, and which it is keeping ready.
#[derive(Clone)]
pub struct WindowState {
    /// Which buffer is on screen, when one is.
    ///
    /// A window with nothing in it is a real state. Closing the last buffer used to conjure an
    /// empty scratch one, which is a file nobody asked for sitting on the buffer line. An empty
    /// window says it is empty.
    pub current: Option<BufferId>,
    /// Which buffers have an editor mounted here, most recently seen first.
    pub mounted: Vec<BufferId>,
    /// How much larger or smaller this window's text is than the setting, in steps of one pixel.
    ///
    /// Per window, and never per application. `<C-+>` in a split asks to read *this* file more
    /// comfortably, and shrinking the status line along with it answers a different question.
    pub font_step: i32,
    /// Which buffers this window shows in their rich form. See [`crate::rich`].
    ///
    /// Per window and per buffer: the same file can be source in one split and a page in
    /// another, and toggling one markdown buffer leaves the next one alone. A `Vec`, because it
    /// can hold at most as many entries as `mounted`.
    pub rich: Vec<BufferId>,
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
    /// Where the keyboard is, for the whole session.
    ///
    /// Made here because this is what makes the first window, so the model has one to name from
    /// the moment it exists. What it holds is more than the windows — the tree and every overlay
    /// are in it — so everything reads it out of the context the session publishes, and this holds
    /// it only to answer "which pane is current".
    focus: crate::focus::Focusing,
    /// The buffer shown before the current one, which `<Leader>bp` goes back to.
    alternate: RwSignal<Option<BufferId>, LocalStorage>,
    /// What the interface is telling the user, shown in the status line until something replaces
    /// it.
    message: RwSignal<Option<Message>, LocalStorage>,
    /// The editor driving each mounted view, by the window and buffer it belongs to.
    ///
    /// No signal. Nothing on screen is decided by which handles exist, and an action that needs
    /// one needs it right now, and not on the next flush.
    handles: RefCell<FxHashMap<(WindowId, BufferId), zgui_editor::EditorHandle>>,
    /// A number that changes whenever an editor arrives or goes away.
    ///
    /// The map itself is no signal, because nothing on screen is decided by which handles exist.
    /// *When* one arrives matters to one thing: the effect that gives the keyboard to the focused
    /// window. A window made by `:split` is focused before its editor has mounted, so that effect
    /// runs, finds no editor and gives up. Without something to wake it, the keyboard stays where
    /// it was and `<C-j>` appears to do nothing until a later split re-runs it.
    mounted: RwSignal<u64, LocalStorage>,
    /// Every file opened this session, most recent first. No signal, for the same reason as the
    /// handles: a picker reads it when it asks, and nothing draws it.
    recent: RefCell<Vec<PathBuf>>,
    /// The reactive owner every buffer's signals are created under.
    ///
    /// A signal belongs to whichever owner was current when it was made, and dies with it. A
    /// buffer is made from wherever the action that opened it was running, and that is a key
    /// handler on some element, whose owner is the pane it is mounted in. `<Leader>th` splits the
    /// window and *then* starts a terminal, so the buffer's signals were created under a pane the
    /// split had just taken apart. They were dead the moment they were made, and the buffer line
    /// panicked the first time it asked one of them for the tab's name.
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
            rich: Vec::new(),
        });

        Self {
            inner: Rc::new(Inner {
                project,
                buffers: RwSignal::new_local(buffers),
                order: RwSignal::new_local(vec![scratch]),
                windows: RwSignal::new_local(windows),
                layout: RwSignal::new_local(Layout::Leaf(window)),
                focus: crate::focus::Focusing::new(window),
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
    /// Every buffer is created through this, and none any other way. See [`Inner::owner`].
    fn owned<T>(&self, make: impl FnOnce() -> T) -> T {
        self.inner.owner.with(make)
    }

    /// The directory everything is relative to.
    pub fn project(&self) -> &Project {
        &self.inner.project
    }

    /// Where the keyboard is.
    pub fn focus(&self) -> &crate::focus::Focusing {
        &self.inner.focus
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
/// If no workspace was provided above this component. That is a wiring mistake, and nothing can
/// carry on from it.
pub fn use_workspace() -> Workspace {
    zgui::reactive::use_local_context::<Workspace>()
        .expect("a workspace is provided at the application root")
}
