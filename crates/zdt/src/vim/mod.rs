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

pub mod whichkey;

mod apply;
mod keymap;
mod keys;
mod read;

use std::cell::RefCell;
use std::rc::Rc;

use zdt_vim::config::merge;
use zdt_vim::effect::{Context, Effect, Scroll, Selection, Step};
use zdt_vim::engine::Engine;
use zdt_vim::keymap::{Keymap, Layered, Resolution};
use zdt_vim::motion::View;
use zdt_vim::notation::{Leaders, parse};
use zdt_vim::{Chord, Mode};
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui_editor::{Clipboard, Command, EditorHandle, InsertPoint, ScrollCmd};

use crate::workspace::Workspace;

/// The keymap the editor ships with.
use crate::assets::KEYMAP as DEFAULTS;

/// How deep a replay may go before it is refused.
///
/// A macro that plays itself is the one way a key can never come back, so it is bounded here as
/// well as in the engine.
const REPLAY_DEPTH: u32 = 64;

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
    keymap: RefCell<Keymap>,
    /// Each region's own keys, in front of the base map: the tree, a picker, a terminal.
    overlays: RefCell<rustc_hash::FxHashMap<String, Keymap>>,
    /// What a region has typed toward one of its own sequences.
    region_keys: RefCell<Vec<Chord>>,
    /// A leap in progress, which takes every key while it is running.
    leaping: crate::leap::Leaping,
    workspace: Workspace,
    settings: crate::settings::Settings,
    /// How deep a replay is, so a macro that plays itself stops.
    depth: std::cell::Cell<u32>,
    // What the interface shows, and nothing else.
    mode: RwSignal<Mode, LocalStorage>,
    pending: RwSignal<String, LocalStorage>,
    recording: RwSignal<Option<char>, LocalStorage>,
}

impl Vim {
    /// The modal layer over `workspace`, with the shipped keymap.
    ///
    /// A keymap that does not read is a bug in the editor, and not in anybody's configuration.
    /// It is reported, and the editor carries on with whatever did read.
    pub fn new(workspace: Workspace, settings: crate::settings::Settings) -> Self {
        let mut keymap = Keymap::new();
        if let Err(problems) = merge(&mut keymap, DEFAULTS, Leaders::default()) {
            for problem in problems {
                tracing::error!("the shipped keymap: {problem}");
            }
        }

        Self {
            inner: Rc::new(Inner {
                engine: RefCell::new(Engine::new()),
                keymap: RefCell::new(keymap),
                overlays: RefCell::new(rustc_hash::FxHashMap::default()),
                region_keys: RefCell::new(Vec::new()),
                leaping: crate::leap::Leaping::new(),
                workspace,
                settings,
                depth: std::cell::Cell::new(0),
                mode: RwSignal::new_local(Mode::Normal),
                pending: RwSignal::new_local(String::new()),
                recording: RwSignal::new_local(None),
            }),
        }
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
