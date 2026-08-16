//! The state machine one key at a time goes through.
//!
//! Keys come in, [`Effect`]s come out, and the whole vim grammar lives in between:
//!
//! ```text
//! ["reg] [count] operator [count] motion|textobject
//! ```
//!
//! Two parts of that stay out of the keymap, because making them rebindable would mean nothing:
//! the digits that make a count, and the `"` that names a register. Everything else, such as `w`,
//! `d` and `iw`, is a row in a file. It resolves through the keymap trie and reaches here as a
//! named action.
//!
//! # What is here
//!
//! Modes, counts, registers, operators, motions, text objects, the visual modes including the
//! block one, marks, the jump list, macros, `.`, and the edits that are commands of their own.
//!
//! The search line and the ex command line live beside this. They are their own input.

use ropey::Rope;
use rustc_hash::FxHashMap;

use crate::action::{Action, Args};
use crate::chord::{Chord, Named};
use crate::effect::{Context, Effect, Scroll, Selection, Step};
use crate::keymap::{Layered, Resolution};
use crate::mode::Mode;
use crate::motion::{self, FindChar, Kind, Target};
use crate::register::{Contents, Name, Registers};
use crate::text;
use crate::textobject;

/// How many times a macro may call another before the engine gives up.
const MACRO_DEPTH: u32 = 64;

/// An operator that has been typed and is waiting for what to apply to.
#[derive(Clone, Debug)]
struct PendingOperator {
    /// Which one.
    action: Action,
    /// The last key of the sequence that started it, which typed again means "this line".
    ///
    /// Vim's doubling rule: `dd`, `yy`, `>>`, `gcc`. It holds the *key*, because `gcc` doubles on
    /// `c` while the operator is `gc`.
    key: Chord,
    /// The count typed before it.
    count: u32,
}

/// A raw key the engine is waiting for, which no keymap gets a say in.
#[derive(Clone, Debug)]
enum Awaiting {
    /// The character `f`, `F`, `t` or `T` is looking for.
    FindChar {
        /// Whether to look backwards.
        backward: bool,
        /// Whether to stop before it.
        till: bool,
        /// How many.
        count: u32,
    },
    /// The character `r` replaces with.
    ReplaceChar {
        /// How many.
        count: u32,
    },
    /// The letter `m` names a mark with.
    SetMark,
    /// The letter a jump to a mark names.
    JumpMark {
        /// Whether to land on the line's first non-blank. The exact place otherwise.
        line: bool,
    },
    /// The letter `"` names a register with.
    Register,
    /// The letter `q` records into.
    RecordMacro,
    /// The letter `@` plays.
    PlayMacro,
}

/// What `.` puts back.
#[derive(Clone, Debug)]
struct Repeat {
    /// The keys that made the change, replayed as if typed.
    keys: Vec<Chord>,
}

/// Where the caret was before a jump.
#[derive(Debug, Default)]
struct JumpList {
    places: Vec<usize>,
    at: usize,
}

impl JumpList {
    /// Remembers `byte` as somewhere to come back to.
    fn push(&mut self, byte: usize) {
        self.places.truncate(self.at);
        if self.places.last() == Some(&byte) {
            return;
        }
        self.places.push(byte);
        // A hundred is more than anybody walks back through, and is what vim keeps.
        if self.places.len() > 100 {
            self.places.remove(0);
        }
        self.at = self.places.len();
    }

    /// One step back, remembering where we were so forward works.
    fn back(&mut self, from: usize) -> Option<usize> {
        if self.at == 0 {
            return None;
        }
        if self.at == self.places.len() {
            self.places.push(from);
        }
        self.at -= 1;
        self.places.get(self.at).copied()
    }

    /// One step forward.
    fn forward(&mut self) -> Option<usize> {
        if self.at + 1 >= self.places.len() {
            return None;
        }
        self.at += 1;
        self.places.get(self.at).copied()
    }
}

/// The modal editor, as one value.
pub struct Engine {
    mode: Mode,
    /// The keys typed toward a binding that has not resolved yet.
    keys: Vec<Chord>,
    /// The count typed before the current command.
    count: Option<u32>,
    /// The register the current command was told to use.
    register: Option<Name>,
    /// The operator waiting for something to apply to.
    operator: Option<PendingOperator>,
    /// A raw key the engine is waiting for.
    awaiting: Option<Awaiting>,
    /// Where the visual selection was started.
    visual_anchor: usize,
    /// Where the visual selection's caret is.
    ///
    /// Kept beside the selections, because a linewise or block selection's ends are somewhere
    /// else. The caret is on a character, and the selection covers whole lines or a rectangle
    /// around it.
    visual_head: usize,
    /// The last key that resolved a binding, which is what an operator doubles on.
    resolved: Chord,
    /// The column a run of `j` and `k` is aiming for.
    goal_column: Option<usize>,
    registers: Registers,
    marks: FxHashMap<char, usize>,
    jumps: JumpList,
    last_find: Option<FindChar>,
    /// Everything typed since the last change started, for `.`.
    recording_change: Vec<Chord>,
    last_change: Option<Repeat>,
    /// The register a macro is being recorded into, and what it has so far.
    recording_macro: Option<(char, Vec<Chord>)>,
    last_macro: Option<char>,
    macro_depth: u32,
    /// Whether the keys arriving are a macro being played, which must not be recorded again.
    replaying: bool,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

mod edits;
mod jumps;
mod modes;
mod motions;
mod objects;
mod operators;
mod ranges;
mod raw;
mod view;

use crate::engine::ranges::{
    block_selections, indent_lines, linewise_caret, operator_range, swap_case,
};

impl Engine {
    /// A fresh engine, in normal mode.
    #[must_use]
    pub fn new() -> Self {
        Self {
            mode: Mode::Normal,
            keys: Vec::new(),
            count: None,
            register: None,
            operator: None,
            awaiting: None,
            visual_anchor: 0,
            visual_head: 0,
            resolved: Chord::char(' '),
            goal_column: None,
            registers: Registers::new(),
            marks: FxHashMap::default(),
            jumps: JumpList::default(),
            last_find: None,
            recording_change: Vec::new(),
            last_change: None,
            recording_macro: None,
            last_macro: None,
            macro_depth: 0,
            replaying: false,
        }
    }

    /// Which mode the editor is in.
    #[must_use]
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The registers, for the picker that lists them.
    #[must_use]
    pub fn registers(&self) -> &Registers {
        &self.registers
    }

    /// Where each mark is, for the picker that lists them.
    ///
    /// In name order, so that `a` reads before `b` however they were set.
    #[must_use]
    pub fn marks(&self) -> Vec<(char, usize)> {
        let mut marks: Vec<(char, usize)> = self
            .marks
            .iter()
            .map(|(name, byte)| (*name, *byte))
            .collect();
        marks.sort_unstable();
        marks
    }

    /// Whether a macro is being recorded, and into which register.
    #[must_use]
    pub fn recording(&self) -> Option<char> {
        self.recording_macro.as_ref().map(|(name, _)| *name)
    }

    /// What has been typed toward a binding that has not resolved, as a keymap would write it.
    ///
    /// What the status line echoes and what which-key is shown for.
    #[must_use]
    pub fn pending_keys(&self) -> &[Chord] {
        &self.keys
    }

    /// The count typed so far, when one was.
    #[must_use]
    pub fn pending_count(&self) -> Option<u32> {
        self.count
    }

    /// The key of the operator that is waiting for something to apply to.
    ///
    /// What the status line echoes while `d` waits for a motion. It also tells which-key to offer
    /// the motions.
    #[must_use]
    pub fn pending_operator(&self) -> Option<Chord> {
        self.operator.as_ref().map(|pending| pending.key)
    }

    /// Puts the engine back in normal mode with nothing pending, which is what `<Esc>` does and
    /// what a buffer switch has to do.
    pub fn reset(&mut self) {
        self.keys.clear();
        self.count = None;
        self.register = None;
        self.operator = None;
        self.awaiting = None;
        self.mode = Mode::Normal;
    }

    /// Takes one key.
    pub fn key(&mut self, chord: Chord, keymap: &Layered<'_>, cx: &Context<'_>) -> Step {
        // A macro records what it is given, before anything decides what it means. The `q` that
        // stops the recording is taken off again below, so a macro does not end by stopping
        // itself when it is played.
        if !self.replaying
            && let Some((_, keys)) = self.recording_macro.as_mut()
        {
            keys.push(chord);
        }
        self.recording_change.push(chord);

        let step = self.dispatch(chord, keymap, cx);

        // Nothing is pending any more, so whatever was typed is a change worth repeating. A key
        // that changed nothing leaves `.` holding the last real change.
        if matches!(step, Step::Consumed(_)) && self.operator.is_none() && self.awaiting.is_none() {
            let changed = step
                .effects()
                .iter()
                .any(|effect| matches!(effect, Effect::Replace(_)));
            if changed {
                self.last_change = Some(Repeat {
                    keys: std::mem::take(&mut self.recording_change),
                });
            } else {
                self.recording_change.clear();
            }
        }

        step
    }

    /// One key, once the recorders have seen it.
    fn dispatch(&mut self, chord: Chord, keymap: &Layered<'_>, cx: &Context<'_>) -> Step {
        if let Some(waiting) = self.awaiting.take() {
            return self.literal(waiting, chord, cx);
        }

        if self.mode.is_inserting() {
            // In insert mode the keymap only gets a say about the keys it has bound, such as
            // `<Esc>`. Everything else is text, and the editor handles it better than this could.
            // It knows about auto-indent, and about grouping a whole insertion into one undo
            // step.
            return match keymap.resolve(self.mode, &[chord]) {
                Resolution::Run(binding) => self.run(&binding.actions.clone(), cx),
                Resolution::Pending(_) | Resolution::None => Step::PassThrough,
            };
        }

        // The grammar's own two prefixes, which no keymap has a say in.
        if self.keys.is_empty() {
            if chord == Chord::char('"') {
                self.awaiting = Some(Awaiting::Register);
                return Step::Pending;
            }
            if let Some(digit) = chord.digit() {
                // A leading `0` is a motion, not a count; a `0` after one is a digit.
                if digit != 0 || self.count.is_some() {
                    self.count = Some(self.count.unwrap_or(0).saturating_mul(10) + digit);
                    return Step::Pending;
                }
            }
        }

        // The doubled operator: `dd`, `yy`, `gcc`.
        if let Some(pending) = self.operator.as_ref()
            && self.keys.is_empty()
            && chord == pending.key
        {
            let pending = self.operator.take().expect("just matched");
            let count = pending.count.saturating_mul(self.count.take().unwrap_or(1));
            return self.apply_linewise_operator(&pending.action, count, cx);
        }

        self.keys.push(chord);
        match keymap.resolve(self.mode, &self.keys) {
            Resolution::Pending(_) => Step::Pending,
            Resolution::None => {
                // Nothing is bound. Whatever was building up is abandoned, which is what makes a
                // mistyped sequence harmless.
                self.keys.clear();
                self.count = None;
                self.register = None;
                self.operator = None;
                Step::nothing()
            }
            Resolution::Run(binding) => {
                let actions = binding.actions.clone();
                // The last key of the sequence, which is what an operator doubles on: `dd`, `yy`,
                // and `gcc`, whose operator is `gc` and whose doubling key is `c`.
                self.resolved = self.keys.last().copied().unwrap_or(chord);
                self.keys.clear();
                self.run(&actions, cx)
            }
        }
    }

    /// Runs what a binding resolved to.
    fn run(&mut self, actions: &[Action], cx: &Context<'_>) -> Step {
        let mut effects = Vec::new();
        for action in actions {
            match self.act(action, cx) {
                Step::Consumed(mut more) => effects.append(&mut more),
                Step::Pending => return Step::Pending,
                Step::PassThrough => return Step::PassThrough,
            }
        }
        Step::Consumed(effects)
    }

    /// Runs one action.
    fn act(&mut self, action: &Action, cx: &Context<'_>) -> Step {
        let count = self.take_count();

        if action.is("motion") {
            return self.motion(action, count, cx);
        }
        if action.is("textobject") {
            return self.text_object(action, count, cx);
        }
        if action.is("operator") {
            return self.operator(action, count, cx);
        }
        if action.is("mode") {
            return self.mode_change(action, count, cx);
        }
        if action.is("visual") {
            return self.visual(action, cx);
        }
        if action.is("edit") {
            return self.edit(action, count, cx);
        }
        if action.is("scroll") {
            return self.scroll(action, count);
        }
        if action.is("mark") {
            return self.mark(action);
        }
        if action.is("jump") {
            return self.jump(action, cx);
        }
        if action.is("macro") {
            return self.macro_action(action);
        }

        // The application's. It knows what a picker and a language server are.
        Step::one(Effect::App(action.clone()))
    }

    /// The count for this command, which is the one typed and the operator's multiplied together.
    fn take_count(&mut self) -> u32 {
        let typed = self.count.take().unwrap_or(1);
        match self.operator.as_ref() {
            Some(pending) => pending.count.saturating_mul(typed),
            None => typed,
        }
    }

    /// Which register this command was told to use.
    fn take_register(&mut self) -> Name {
        self.register.take().unwrap_or(Name::UNNAMED)
    }
}
