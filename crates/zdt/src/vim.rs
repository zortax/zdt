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
    /// A keymap that does not read is a bug in the editor rather than in anybody's configuration,
    /// so it is reported and the editor carries on with whatever did read.
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
        let layered = Layered::plain(&keymap);

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

    /// Everything bound in normal mode, as the keys that reach it and what it is called.
    ///
    /// Owned rather than borrowed, because the keymap is behind a `RefCell` and a picker holds
    /// what it lists for as long as it is open.
    #[must_use]
    pub fn bindings(&self) -> Vec<Bound> {
        let keymap = self.inner.keymap.borrow();
        keymap
            .bindings(Mode::Normal)
            .into_iter()
            .map(|(keys, binding)| Bound {
                keys: zdt_vim::notation::format(&keys),
                actions: binding.actions.clone(),
                description: binding.description.clone(),
            })
            .collect()
    }

    /// What is in each register that has anything in it.
    #[must_use]
    pub fn registers(&self) -> Vec<(char, String)> {
        let engine = self.inner.engine.borrow();
        engine
            .registers()
            .occupied()
            .into_iter()
            .map(|(name, contents)| (name, contents.text.clone()))
            .collect()
    }

    /// Where each mark is, as a byte offset into the buffer it was set in.
    #[must_use]
    pub fn marks(&self) -> Vec<(char, usize)> {
        self.inner.engine.borrow().marks()
    }

    /// Carries out one action, as though a key had asked for it.
    ///
    /// What a picker of commands does with the row somebody chose.
    pub fn run(&self, action: &zdt_vim::Action) {
        let handle = self.inner.workspace.current_handle();
        crate::actions::run(&self.inner.workspace, self, action, handle.as_ref());
    }

    /// Puts the keymap back to the one the editor ships with.
    ///
    /// What a reload does before reading a person's file again: layering the new file onto what is
    /// already there would leave behind every row they have since deleted.
    pub fn reset_keymap(&self) {
        let mut keymap = Keymap::new();
        if let Err(problems) = merge(&mut keymap, DEFAULTS, Leaders::default()) {
            for problem in problems {
                tracing::error!("the shipped keymap: {problem}");
            }
        }
        *self.inner.keymap.borrow_mut() = keymap;
    }

    /// Reads more keymap text on top of what is there, which is what a user's file is.
    pub fn merge_keymap(&self, text: &str, leaders: Leaders) -> Result<(), Vec<String>> {
        let mut keymap = self.inner.keymap.borrow_mut();
        merge(&mut keymap, text, leaders)
            .map_err(|problems| problems.iter().map(ToString::to_string).collect())
    }

    /// Puts a region's own keys in front of the base map, under `name`.
    pub fn set_overlay(&self, name: &str, keymap: Keymap) {
        self.inner
            .overlays
            .borrow_mut()
            .insert(name.to_owned(), keymap);
    }

    /// Takes them off again.
    pub fn clear_overlay(&self, name: &str) {
        self.inner.overlays.borrow_mut().remove(name);
    }

    /// Reads a region's keymap out of text, and puts it in front under `name`.
    ///
    /// A region's keys are a file like every other keymap, so a person can change them: `extra` is
    /// read after `text`, which is where their own file goes.
    pub fn load_overlay(
        &self,
        name: &str,
        text: &str,
        extra: Option<&str>,
    ) -> Result<(), Vec<String>> {
        let mut keymap = Keymap::new();
        let mut problems: Vec<String> = Vec::new();
        for source in std::iter::once(text).chain(extra) {
            if let Err(found) = merge(&mut keymap, source, Leaders::default()) {
                problems.extend(found.iter().map(ToString::to_string));
            }
        }
        // Whatever did read is still installed: a region with most of its keys is more use than
        // one with none, and the problems are reported either way.
        self.set_overlay(name, keymap);
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
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
        // Documentation stays up until the next key, whatever that key is. It takes no keyboard
        // of its own, so this is the only thing that has to know it is there.
        if let Some(hover) = zgui::reactive::use_local_context::<crate::ui::hover::Hover>()
            && hover.is_showing()
        {
            hover.hide();
        }

        // Labelled tabs take the next key, whatever it is: every letter is a label or the end of
        // the labelling, so a keymap answering one would put some tabs out of reach.
        if let Some(tabs) = zgui::reactive::use_local_context::<crate::tabpick::TabPick>()
            && tabs.is_running()
        {
            let character = match chord.key {
                zdt_vim::chord::Key::Char(character) if chord.mods.is_empty() => Some(character),
                _ => None,
            };
            return tabs.key(character);
        }

        // A leap in progress takes every key: once it has started, each one is either a character
        // it is aiming at or a label, and a keymap that answered any of them would put some
        // letters out of reach.
        if self.inner.leaping.is_running() {
            return self.leap_key(chord, handle);
        }

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

    /// Starts a leap looking `direction`.
    pub fn start_leap(&self, direction: zdt_vim::leap::Direction) {
        self.inner.leaping.start(direction);
    }

    /// The leap layer, for the overlay that draws its labels.
    #[must_use]
    pub fn leaping(&self) -> crate::leap::Leaping {
        self.inner.leaping.clone()
    }

    /// One key while a leap is in progress.
    ///
    /// Always answers `true`: every key belongs to the leap while one is running, including the
    /// one that ends it.
    fn leap_key(&self, chord: Chord, handle: &EditorHandle) -> bool {
        use zdt_vim::chord::Key;

        // Only a plain character narrows or chooses. A chord with a modifier on it — and `<Esc>`,
        // which is how anybody expects to get out — ends the leap.
        let character = match chord.key {
            Key::Char(character) if chord.mods.is_empty() => Some(character),
            _ => None,
        };

        let took = handle.query(|snapshot| {
            let rope = snapshot.rope();
            let window = snapshot.visible_byte_range();
            let caret = snapshot.selections().primary().head;
            self.inner
                .leaping
                .key(character, |pair, direction, alphabet| {
                    zdt_vim::leap::landings(rope, window, caret, pair, direction, alphabet)
                })
        });

        if let crate::leap::Took::Landed(byte) = took {
            let step = handle.query(|snapshot| {
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
                self.inner.engine.borrow_mut().leap_to(byte, &context)
            });
            if let Step::Consumed(effects) = step {
                self.carry_out(effects, handle);
            }
        }
        self.publish();
        true
    }

    /// Takes one key for a region that is not an editor: the tree, a picker, a terminal.
    ///
    /// The same keymap with that region's rows in front, resolved in normal mode — a region has no
    /// modes of its own — and no editor to apply anything to. Answers whether the key was used.
    pub fn key_in_region(&self, chord: Chord, region: &str) -> bool {
        self.key_in_region_as(chord, region, Mode::Normal)
    }

    /// The same, resolved in `mode`.
    ///
    /// For a terminal being typed into: almost nothing is bound in terminal mode, so almost every
    /// key falls through to the program — which is the point. What *is* bound there is what vim's
    /// own `maps.t` binds, and it wins over the program deliberately.
    pub fn key_in_region_as(&self, chord: Chord, region: &str, mode: Mode) -> bool {
        let overlay = self.inner.overlays.borrow();
        let map = overlay.get(region);
        let keymap = self.inner.keymap.borrow();
        // A region with no keymap of its own still gets the base map: a terminal in normal mode
        // has no rows of its own, and `<Leader>ff` from inside one has to work.
        let layered = match map {
            Some(map) => Layered::new(map, &keymap),
            None => Layered::plain(&keymap),
        };

        // A region's keys have no grammar: no counts, no operators, nothing to hold between
        // presses but the sequence itself.
        let mut keys = self.inner.region_keys.borrow_mut();
        keys.push(chord);

        match layered.resolve(mode, &keys) {
            Resolution::Pending(_) => {
                drop(keys);
                self.publish_region();
                true
            }
            Resolution::None => {
                keys.clear();
                drop(keys);
                self.publish_region();
                false
            }
            Resolution::Run(binding) => {
                let actions = binding.actions.clone();
                keys.clear();
                drop(keys);
                drop(overlay);
                drop(keymap);
                self.publish_region();
                for action in &actions {
                    crate::actions::run(&self.inner.workspace, self, action, None);
                }
                true
            }
        }
    }

    /// Echoes what a region has typed so far, so which-key and the status line follow it too.
    fn publish_region(&self) {
        let keys = self.inner.region_keys.borrow();
        let pending = zdt_vim::notation::format(&keys);
        if self.inner.pending.get_untracked() != pending {
            self.inner.pending.set(pending);
        }
    }

    /// One key, without publishing what changed.
    fn step(&self, chord: Chord, handle: &EditorHandle) -> Step {
        let engine = &self.inner.engine;
        let keymap = self.inner.keymap.borrow();
        let layered = Layered::plain(&keymap);

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
    pub fn enter(&self, mode: Mode, handle: &EditorHandle) {
        use zdt_core::config::LineNumbers;
        use zgui_editor::{CursorStyle, GutterMode};

        handle.set_cursor_style(match mode {
            Mode::Insert | Mode::Command => CursorStyle::Bar,
            Mode::Replace => CursorStyle::Underline,
            _ => CursorStyle::Block,
        });

        // Relative numbering is for moving around; while typing, the absolute number is the one
        // worth having. A person who asked for absolute or for none gets what they asked for in
        // every mode.
        let numbers = self
            .inner
            .settings
            .with_untracked(|config| config.editor.line_numbers);
        handle.set_gutter(match (numbers, mode.is_inserting()) {
            (LineNumbers::None, _) => GutterMode::None,
            (LineNumbers::Absolute, _) | (LineNumbers::Relative, true) => GutterMode::Absolute,
            (LineNumbers::Relative, false) => GutterMode::Relative,
        });
    }

    /// Puts the editor back into whatever the current mode looks like.
    ///
    /// What a settings change calls: the gutter and the caret are decided by the mode and the
    /// settings together, and only one of the two has just moved.
    pub fn refresh(&self, handle: &EditorHandle) {
        self.enter(self.inner.engine.borrow().mode(), handle);
    }

    /// An action the engine handed back because the application owns it.
    fn app_action(&self, action: &zdt_vim::Action, handle: &EditorHandle) {
        if action.name == "vim.replay" {
            let keys = action.args.str("keys").unwrap_or_default().to_owned();
            self.replay(&keys, handle);
            return;
        }
        crate::actions::run(&self.inner.workspace, self, action, Some(handle));
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
