//! Where the keyboard is.
//!
//! One answer for the whole session, and every region reads it. A panel is focused because the
//! model says so, and never because it wrote a flag of its own. Two regions cannot both believe
//! they have the keyboard, because there is one value and it has one variant.
//!
//! # The three tiers
//!
//! A [`Region`] is one of the two places `<C-w>h` walks between: the panes and the file tree. An
//! [`Overlay`] is something over them that takes the keys, and gives the region back when it goes.
//! [`Focus`] is the two of them together, and is the only thing anything reads.
//!
//! # Taking the keys and taking the keyboard
//!
//! These are two different things. A picker takes both: its field holds the caret, so the editor
//! underneath goes quiet. Documentation, suggestions, tab labels and a leap take only the keys, and
//! the caret stays where it was. Which is which is whether anything registers a [`Sink`] for it, so
//! a layer that draws no input needs no sink and gets the behaviour for free.
//!
//! # What is not here
//!
//! Whether an overlay is open is the overlay's own state, and stays there. What this holds is the
//! consequence: that while it is open, it has the keys.

pub mod claim;
pub mod mode;
pub mod project;

mod read;

use std::cell::RefCell;
use std::rc::Rc;

use rustc_hash::FxHashMap;
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

use crate::workspace::{BufferId, WindowId};

/// One of the two places the window commands walk between.
///
/// Everything else is a layer over one of them.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Region {
    /// The panes.
    Panes,
    /// The file tree.
    Tree,
}

/// Something over the regions that takes the **keyboard**.
///
/// Each has somewhere to type, is dismissible, and hands the region back, which is what makes it an
/// overlay rather than a third region.
///
/// # What does not belong here
///
/// The documentation panel, the suggestion popup, the letters on the buffer line and a leap in
/// progress read keys *first* and take the keyboard from nobody: the caret stays in the editor, and
/// the editor stays in whatever mode it was in. Putting one of them here would make the status line
/// say NORMAL while somebody is typing a word in insert mode. They are handled where they belong,
/// at the top of [`Vim::key`](crate::vim::Vim::key).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Overlay {
    /// The floating terminal.
    Float(BufferId),
    /// A picker.
    Picker,
    /// The one-line question.
    Prompt,
    /// The rename box over a symbol.
    Rename,
    /// The file tree's field, over the row it is about.
    TreeField,
    /// The `:` line.
    CommandLine,
    /// The settings, floating.
    Settings,
    /// The git panel, floating.
    GitModal,
}

/// Where the keyboard is.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Focus {
    /// A window's contents: the editor, the terminal or the panel in it.
    Window(WindowId),
    /// The file tree.
    Tree,
    /// A layer over whichever of those had it.
    Overlay(Overlay),
}

/// One thing that can hold the keyboard, as the projector names it.
///
/// Policy is [`Focus`]; this is the plumbing. A window resolves to the buffer it is showing,
/// because a window keeps several views mounted and only one of them is on screen.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Spot {
    /// One buffer's view in one window.
    Buffer(WindowId, BufferId),
    /// The file tree.
    Tree,
    /// A layer.
    Overlay(Overlay),
}

/// How the keyboard reaches a spot.
#[derive(Clone)]
pub enum Sink {
    /// An editor, which takes it through its handle.
    Editor(zgui_editor::EditorHandle),
    /// An element that takes focus itself.
    Node(zgui::view::NodeRef),
    /// The first thing inside an element that takes focus.
    ///
    /// For a region whose own box is layout around something else. A terminal draws its own
    /// focusable element with its own key handlers on it, and the box zdt puts around it holds a
    /// class and a place on the screen. Focusing the box would focus nothing at all.
    Inside(zgui::view::NodeRef),
}

impl Sink {
    /// Gives it the keyboard.
    ///
    /// Reads nothing reactive. A node whose view has gone is unbound and answers by doing nothing,
    /// which is the right answer for a spot that is no longer on screen.
    pub fn focus(&self) {
        match self {
            Self::Editor(handle) => handle.focus(),
            Self::Node(node) => node.focus(),
            Self::Inside(node) => {
                node.focus_move(zgui::view::FocusMove::First);
            }
        }
    }
}

/// The focus of one session.
///
/// Cloning one is cloning a handle.
#[derive(Clone)]
pub struct Focusing {
    inner: Rc<Inner>,
}

struct Inner {
    /// Which pane is the current one.
    ///
    /// Survives a trip into the tree and every overlay, because something that closes has to give
    /// the keyboard back to somewhere. Always names a window: the workspace makes one before it
    /// makes this, so there is no moment where the answer is nothing.
    window: RwSignal<WindowId, LocalStorage>,
    /// Whether the keyboard is in the panes or in the tree.
    region: RwSignal<Region, LocalStorage>,
    /// What is layered over it, innermost last.
    stack: RwSignal<Vec<Overlay>, LocalStorage>,
    /// Where each spot's keyboard goes.
    ///
    /// No signal. Nothing on screen is decided by which sinks exist, and the projector needs one
    /// right now and not on the next flush.
    sinks: RefCell<FxHashMap<Spot, Sink>>,
    /// A number that changes whenever a sink arrives or goes away.
    ///
    /// What the projector reads first, so a region that registers after it was focused still ends
    /// up with the keyboard.
    mounted: RwSignal<u64, LocalStorage>,
}

impl Focusing {
    /// The keyboard in `window`, with nothing over it.
    ///
    /// Made by the workspace, which is what guarantees there is a window to name. Everything else
    /// reads it out of the context the session publishes.
    #[must_use]
    pub fn new(window: WindowId) -> Self {
        Self {
            inner: Rc::new(Inner {
                window: RwSignal::new_local(window),
                region: RwSignal::new_local(Region::Panes),
                stack: RwSignal::new_local(Vec::new()),
                sinks: RefCell::new(FxHashMap::default()),
                mounted: RwSignal::new_local(0),
            }),
        }
    }

    // ---- Moving it ---------------------------------------------------------------------------

    /// Makes `window` the current pane and gives it the keyboard.
    pub fn enter_window(&self, window: WindowId) {
        self.set_window(window);
        self.enter_panes();
    }

    /// Gives the keyboard to the panes, whichever one is current.
    pub fn enter_panes(&self) {
        if self.inner.region.get_untracked() != Region::Panes {
            self.inner.region.set(Region::Panes);
        }
    }

    /// Gives the keyboard to the file tree.
    pub fn enter_tree(&self) {
        if self.inner.region.get_untracked() != Region::Tree {
            self.inner.region.set(Region::Tree);
        }
    }

    /// Says the current pane changed without the keyboard moving.
    ///
    /// What a split closing does: the pane that is current has to be one that still exists, and
    /// somebody reading the tree at the time stays in it.
    pub fn set_window(&self, window: WindowId) {
        if self.inner.window.get_untracked() != window {
            self.inner.window.set(window);
        }
    }

    // ---- Layers over it ----------------------------------------------------------------------

    /// Says `overlay` has the keys.
    ///
    /// Called from [`claim`](crate::focus::claim::claim) alone, which is what keeps the stack
    /// balanced.
    pub(super) fn push(&self, overlay: Overlay) {
        self.inner.stack.try_update(|stack| {
            if !stack.contains(&overlay) {
                stack.push(overlay);
            }
        });
    }

    /// Takes it off again.
    ///
    /// Writes and never reads: this runs while a scope is being disposed of, where that scope's own
    /// signals have gone.
    pub(super) fn pop(&self, overlay: Overlay) {
        self.inner.stack.try_update(|stack| {
            stack.retain(|held| *held != overlay);
        });
    }

    // ---- Where the keyboard goes ---------------------------------------------------------------

    /// Says how `spot` takes the keyboard.
    pub fn register(&self, spot: Spot, sink: Sink) {
        self.inner.sinks.borrow_mut().insert(spot, sink);
        self.bump();
    }

    /// Forgets it, which a view does as it unmounts.
    pub fn forget(&self, spot: Spot) {
        let had = self.inner.sinks.borrow_mut().remove(&spot).is_some();
        if had {
            self.bump();
        }
    }

    /// Asks the model to put the keyboard back where it says it is.
    ///
    /// For the places that take it away without saying so: a dragged divider, a library control
    /// that is a tab stop.
    pub fn reproject(&self) {
        self.bump();
    }

    fn bump(&self) {
        self.inner
            .mounted
            .try_update(|revision| *revision = revision.wrapping_add(1));
    }
}

/// Puts the focus where every component can find it.
pub fn provide(focus: Focusing) {
    zgui::reactive::provide_local_context(focus);
}

/// The focus, from inside a component.
///
/// # Panics
///
/// If none was provided above this component. That is a wiring mistake, and nothing can carry on
/// from it.
#[must_use]
pub fn use_focus() -> Focusing {
    zgui::reactive::use_local_context::<Focusing>().expect("a focus is provided at the root")
}

/// The focus, when there is one.
#[must_use]
pub fn try_use_focus() -> Option<Focusing> {
    zgui::reactive::use_local_context::<Focusing>()
}
