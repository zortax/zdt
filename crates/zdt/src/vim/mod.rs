//! The modal layer, joined to the editor.
//!
//! The engine is pure. Keys go in, effects come out, and it has never heard of an editor. This is
//! the seam. It reads what the editor looks like right now, hands the engine a key, and turns what
//! comes back into commands.
//!
//! # Why the state is not in signals
//!
//! A keystroke is the hottest path there is, and the mode, the pending count, the registers and
//! the macro recorder change on almost every one. They live in a plain `RefCell`. Only the three
//! things the status line shows are signals, so typing wakes the mode block and nothing else.
//!
//! # What is here and what is not
//!
//! What is being typed is here, and belongs to one session. What the keys *mean* is
//! [`crate::keymaps`], and is the same everywhere.

pub mod surface;
pub mod whichkey;

mod apply;
mod keys;
mod read;

use std::cell::RefCell;
use std::rc::Rc;

use zdt_vim::effect::{Effect, Scroll, Step, Visual};
use zdt_vim::engine::Engine;
use zdt_vim::keymap::Resolution;
use zdt_vim::notation::{Leaders, parse};
use zdt_vim::{Chord, Mode};
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui_editor::{
    Band, Caret, Clipboard, Command, Decoration, EditorHandle, InsertPoint, Overlay, ScrollCmd,
};

use crate::keymaps::Keymaps;
use crate::terminals::normal::Scrollback;
use crate::workspace::Workspace;

pub use crate::vim::keys::Answer;
pub use crate::vim::surface::Surface;

/// How deep a replay may go before it is refused.
///
/// A macro that plays itself is the one way a key can never come back, so it is bounded here as
/// well as in the engine.
const REPLAY_DEPTH: u32 = 64;

/// A region part-way through one of its own sequences.
///
/// The region is a `&'static str` because every caller passes a literal or a constant, which makes
/// this `Copy` and costs nothing to keep.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Typing {
    /// The region, as the keymaps hold it.
    region: &'static str,
    /// The mode its keys resolve in.
    mode: Mode,
}

/// One way a part-typed sequence could carry on, as which-key shows it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Continuation {
    /// The key, written the way a keymap writes it.
    pub keys: String,
    /// What it leads to.
    pub label: String,
    /// Whether it is a whole binding. Another prefix otherwise.
    pub runs: bool,
}

/// The modal layer.
///
/// Cloning one is cloning a handle: every clone drives the same engine, which is what lets the
/// key filter, the status line and the which-key panel all be given one.
#[derive(Clone)]
pub struct Vim {
    inner: Rc<Inner>,
}

struct Inner {
    engine: RefCell<Engine>,
    /// What every key means. Shared with every other session.
    keymaps: Keymaps,
    /// What a region has typed toward one of its own sequences.
    region_keys: RefCell<Vec<Chord>>,
    /// Which region is part-way through a sequence of its own, and in which mode.
    ///
    /// What which-key resolves against. A region's keys have no grammar and are no business of the
    /// engine's, so the engine's pending keys say nothing about them.
    typing: std::cell::Cell<Option<Typing>>,
    /// A leap in progress, which takes every key while it is running.
    leaping: crate::leap::Leaping,
    workspace: Workspace,
    settings: crate::settings::Settings,
    /// How deep a replay is, so a macro that plays itself stops.
    depth: std::cell::Cell<u32>,
    /// What is waiting to put a flash out.
    flash: RefCell<Option<zgui::view::time::TimeoutHandle>>,
    // What the interface shows, and nothing else.
    mode: RwSignal<Mode, LocalStorage>,
    pending: RwSignal<String, LocalStorage>,
    recording: RwSignal<Option<char>, LocalStorage>,
}

impl Vim {
    /// The modal layer over `workspace`, reading `keymaps`.
    pub fn new(
        workspace: Workspace,
        settings: crate::settings::Settings,
        keymaps: Keymaps,
    ) -> Self {
        Self {
            inner: Rc::new(Inner {
                engine: RefCell::new(Engine::new()),
                keymaps,
                region_keys: RefCell::new(Vec::new()),
                typing: std::cell::Cell::new(None),
                leaping: crate::leap::Leaping::new(),
                workspace,
                settings,
                depth: std::cell::Cell::new(0),
                flash: RefCell::new(None),
                mode: RwSignal::new_local(Mode::Normal),
                pending: RwSignal::new_local(String::new()),
                recording: RwSignal::new_local(None),
            }),
        }
    }

    /// What every key means.
    #[must_use]
    pub fn keymaps(&self) -> &Keymaps {
        &self.inner.keymaps
    }
}

/// The modal layer, from inside a component.
///
/// # Panics
///
/// If none was provided above this component. That is a wiring mistake, and nothing can carry on
/// from it.
pub fn use_vim() -> Vim {
    zgui::reactive::use_local_context::<Vim>().expect("a vim layer is provided at the root")
}

/// One row of the keymap, as a picker lists it.
#[derive(Clone, PartialEq, Debug)]
pub struct Bound {
    /// The keys that reach it, in the notation a keymap is written in.
    pub keys: String,
    /// What it does.
    pub actions: Vec<zdt_vim::Action>,
    /// What the keymap calls it.
    pub description: String,
}
