//! The modal layer, joined to the editor.
//!
//! The engine is pure: keys go in, effects come out, and it has never heard of an editor. This is
//! the seam — it reads what the editor looks like right now, hands the engine a key, and turns
//! what comes back into commands.
//!
//! # Why the state is not in signals
//!
//! A keystroke is the hottest path there is, and the mode, the pending count, the registers and
//! the macro recorder change on almost every one of them. They live in a plain `RefCell`, and only
//! the three things the status line actually shows are signals — so typing wakes the mode block
//! and nothing else.

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
const DEFAULTS: &str = include_str!("../../../assets/keymap.toml");

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
    /// Whether it is a whole binding rather than another prefix.
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
    /// A region's own keys, in front of the base map: the tree, a picker, a terminal.
    overlay: RefCell<Option<(String, Keymap)>>,
    workspace: Workspace,
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
    /// A keymap that does not read is a bug in the editor rather than in anybody's configuration,
    /// so it is reported and the editor carries on with whatever did read.
    pub fn new(workspace: Workspace) -> Self {
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
                overlay: RefCell::new(None),
                workspace,
                depth: std::cell::Cell::new(0),
                mode: RwSignal::new_local(Mode::Normal),
                pending: RwSignal::new_local(String::new()),
                recording: RwSignal::new_local(None),
            }),
        }
    }

    /// Which mode the editor is in. Tracked.
    pub fn mode(&self) -> Mode {
        self.inner.mode.get()
    }

    /// Which mode the editor is in, without subscribing.
    pub fn mode_untracked(&self) -> Mode {
        self.inner.mode.get_untracked()
    }

    /// What has been typed toward a binding that has not resolved. Tracked.
    pub fn pending(&self) -> String {
        self.inner.pending.get()
    }

    /// Which register a macro is being recorded into. Tracked.
    pub fn recording(&self) -> Option<char> {
        self.inner.recording.get()
    }

    /// What could come next, when a sequence is part-way through. Tracked.
    ///
    /// Answered as owned rows rather than borrowed ones because the keymap is behind a `RefCell`
    /// and which-key draws from a reactive hole, which is not somewhere a borrow can be held.
    ///
    /// Untracked: what wakes which-key is the pending signal, which the panel watches for itself.
    pub fn continuations(&self) -> Vec<Continuation> {
        let engine = self.inner.engine.borrow();
        let keys = engine.pending_keys();
        // An operator waiting for something to apply to has typed nothing yet, and the motions are
        // exactly what somebody who paused after `d` is looking for.
        if keys.is_empty() && engine.pending_operator().is_none() {
            return Vec::new();
        }

        let keymap = self.inner.keymap.borrow();
        let overlay = self.inner.overlay.borrow();
        let layered = match overlay.as_ref() {
            Some((_, map)) => Layered::new(map, &keymap),
            None => Layered::plain(&keymap),
        };

        match layered.resolve(engine.mode(), keys) {
            Resolution::Pending(next) => next
                .into_iter()
                .map(|one| Continuation {
                    keys: zdt_vim::notation::format(&[one.chord]),
                    label: one.label.to_owned(),
                    runs: one.runs,
                })
                .collect(),
            Resolution::Run(_) | Resolution::None => Vec::new(),
        }
    }

    /// Reads more keymap text on top of what is there, which is what a user's file is.
    pub fn merge_keymap(&self, text: &str, leaders: Leaders) -> Result<(), Vec<String>> {
        let mut keymap = self.inner.keymap.borrow_mut();
        merge(&mut keymap, text, leaders)
            .map_err(|problems| problems.iter().map(ToString::to_string).collect())
    }

    /// Puts a region's own keys in front of the base map.
    pub fn set_overlay(&self, name: &str, keymap: Keymap) {
        *self.inner.overlay.borrow_mut() = Some((name.to_owned(), keymap));
    }

    /// Takes them off again, if `name` is what is there.
    pub fn clear_overlay(&self, name: &str) {
        let mut overlay = self.inner.overlay.borrow_mut();
        if overlay.as_ref().is_some_and(|(held, _)| held == name) {
            *overlay = None;
        }
    }

    /// Puts the engine back in normal mode, which a buffer or window switch has to do.
    pub fn reset(&self) {
        self.inner.engine.borrow_mut().reset();
        self.publish();
    }

    /// Takes one key. Answers whether the editor should be left out of it.
    ///
    /// This is what an editor's key filter is: `true` means the key is used up.
    pub fn key(&self, chord: Chord, handle: &EditorHandle) -> bool {
        let step = self.step(chord, handle);
        self.publish();
        match step {
            Step::Consumed(effects) => {
                self.carry_out(effects, handle);
                true
            }
            Step::Pending => true,
            Step::PassThrough => false,
        }
    }

    /// One key, without publishing what changed.
    fn step(&self, chord: Chord, handle: &EditorHandle) -> Step {
        let engine = &self.inner.engine;
        let keymap = self.inner.keymap.borrow();
        let overlay = self.inner.overlay.borrow();
        let layered = match overlay.as_ref() {
            Some((_, map)) => Layered::new(map, &keymap),
            None => Layered::plain(&keymap),
        };

        handle.query(|snapshot| {
            let selections: Vec<Selection> = snapshot
                .selections()
                .iter()
                .map(|selection| Selection::new(selection.anchor, selection.head))
                .collect();
            let visible = snapshot.visible_lines();
            let context = Context {
                rope: snapshot.rope(),
                selections: &selections,
                view: View {
                    top_line: visible.start,
                    height: visible.len().max(1),
                },
            };
            engine.borrow_mut().key(chord, &layered, &context)
        })
    }

    /// Publishes the three things the interface shows.
    fn publish(&self) {
        let engine = self.inner.engine.borrow();
        let mode = engine.mode();
        if self.inner.mode.get_untracked() != mode {
            self.inner.mode.set(mode);
        }
        let typed = zdt_vim::notation::format(engine.pending_keys());
        let waiting = engine
            .pending_operator()
            .map(|chord| zdt_vim::notation::format(&[chord]))
            .unwrap_or_default();
        let pending = match engine.pending_count() {
            Some(count) => format!("{count}{waiting}{typed}"),
            None => format!("{waiting}{typed}"),
        };
        if self.inner.pending.get_untracked() != pending {
            self.inner.pending.set(pending);
        }
        let recording = engine.recording();
        if self.inner.recording.get_untracked() != recording {
            self.inner.recording.set(recording);
        }
    }

    /// Does what the engine asked for.
    fn carry_out(&self, effects: Vec<Effect>, handle: &EditorHandle) {
        for effect in effects {
            match effect {
                Effect::Select(selections) => {
                    let selections: Vec<zgui_editor::Selection> = selections
                        .iter()
                        .map(|one| zgui_editor::Selection::new(one.anchor, one.head))
                        .collect();
                    if !selections.is_empty() {
                        handle.command(Command::SetSelections {
                            selections,
                            primary: 0,
                        });
                    }
                }
                Effect::Replace(replacements) => {
                    handle.command(Command::ReplaceRanges(replacements));
                }
                Effect::Undo => handle.command(Command::Undo),
                Effect::Redo => handle.command(Command::Redo),
                Effect::Scroll(scroll) => handle.command(Command::Scroll(match scroll {
                    Scroll::Center => ScrollCmd::CursorCenter,
                    Scroll::Top => ScrollCmd::CursorTop,
                    Scroll::Bottom => ScrollCmd::CursorBottom,
                    Scroll::Lines(lines) => ScrollCmd::Lines(f64::from(lines)),
                    Scroll::EnsureVisible => ScrollCmd::EnsureCursorVisible,
                })),
                Effect::Mode(mode) => self.enter(mode, handle),
                Effect::SetClipboard { primary, .. } => {
                    // The editor holds the clipboards, and copying what is selected is what it
                    // already does — so the engine's own copy is not repeated here.
                    handle.command(Command::Copy(if primary {
                        Clipboard::Primary
                    } else {
                        Clipboard::Standard
                    }));
                }
                Effect::ReadClipboard { primary, before } => {
                    if !before {
                        handle.command(Command::Move {
                            motion: zgui_editor::Motion::Right,
                            count: 1,
                            extend: false,
                        });
                    }
                    let _ = InsertPoint::AtCarets;
                    handle.command(Command::Paste(if primary {
                        Clipboard::Primary
                    } else {
                        Clipboard::Standard
                    }));
                }
                Effect::App(action) => self.app_action(&action, handle),
                Effect::Say(text) => self.inner.workspace.say(text),
                Effect::Complain(text) => self.inner.workspace.complain(text),
            }
        }
    }

    /// What the editor looks like in `mode`.
    fn enter(&self, mode: Mode, handle: &EditorHandle) {
        use zgui_editor::{CursorStyle, GutterMode};
        handle.set_cursor_style(match mode {
            Mode::Insert | Mode::Command => CursorStyle::Bar,
            Mode::Replace => CursorStyle::Underline,
            _ => CursorStyle::Block,
        });
        // Relative numbering is for moving around; while typing, the absolute number is the one
        // worth having.
        handle.set_gutter(if mode.is_inserting() {
            GutterMode::Absolute
        } else {
            GutterMode::Relative
        });
    }

    /// An action the engine handed back because the application owns it.
    fn app_action(&self, action: &zdt_vim::Action, handle: &EditorHandle) {
        if action.name == "vim.replay" {
            let keys = action.args.str("keys").unwrap_or_default().to_owned();
            self.replay(&keys, handle);
            return;
        }
        crate::actions::run(&self.inner.workspace, self, action, handle);
    }

    /// Plays `keys` as though they had been typed.
    ///
    /// One key at a time with the editor read again between them, because a macro that edits and
    /// then moves needs the text as it is after the edit rather than as it was when it started.
    pub fn replay(&self, keys: &str, handle: &EditorHandle) {
        if self.inner.depth.get() >= REPLAY_DEPTH {
            self.inner
                .workspace
                .complain("macros are nested too deeply");
            return;
        }
        let Ok(chords) = parse(keys, Leaders::default()) else {
            return;
        };

        self.inner.depth.set(self.inner.depth.get() + 1);
        self.inner.engine.borrow_mut().begin_replay();
        for chord in chords {
            let step = self.step(chord, handle);
            match step {
                Step::Consumed(effects) => self.carry_out(effects, handle),
                Step::Pending => {}
                // A key the engine did not want, replayed: it is text, and the editor's default
                // handling is what typing it would have done.
                Step::PassThrough => {
                    if let Some(character) = chord.inserted() {
                        handle.command(Command::Insert(character.to_string()));
                    }
                }
            }
        }
        self.inner.engine.borrow_mut().end_replay();
        self.inner
            .depth
            .set(self.inner.depth.get().saturating_sub(1));
        self.publish();
    }
}

/// The modal layer, from inside a component.
///
/// # Panics
///
/// If none was provided above this component, which is a wiring mistake rather than a state
/// anything can carry on from.
pub fn use_vim() -> Vim {
    zgui::reactive::use_local_context::<Vim>().expect("a vim layer is provided at the root")
}
