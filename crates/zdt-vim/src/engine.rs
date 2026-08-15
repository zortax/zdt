//! The state machine one key at a time goes through.
//!
//! Keys come in, [`Effect`]s come out, and the whole vim grammar lives in between:
//!
//! ```text
//! ["reg] [count] operator [count] motion|textobject
//! ```
//!
//! Two parts of that are *not* in the keymap, because making them rebindable would mean nothing:
//! the digits that make a count, and the `"` that names a register. Everything else — `w`, `d`,
//! `iw` — is a row in a file, resolved through the keymap trie, and reaches here as a named
//! action.
//!
//! # What is here and what is not
//!
//! Here: modes, counts, registers, operators, motions, text objects, the visual modes including
//! the block one, marks, the jump list, macros, `.`, and the edits that are commands of their own.
//! Not here: the search line and the ex command line, which are their own input and live beside
//! this.

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
    /// Vim's doubling rule: `dd`, `yy`, `>>`, `gcc`. It is the *key* rather than the action
    /// because `gcc` doubles on `c` while the operator is `gc`.
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
        /// Whether to land on the line's first non-blank rather than the exact place.
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
    /// Kept beside the selections rather than read back out of them, because a linewise or block
    /// selection's ends are not where the caret is — the caret is on a character and the selection
    /// covers whole lines or a rectangle around it.
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
    /// What the status line echoes while `d` waits for a motion, and what tells which-key to
    /// offer the motions rather than nothing.
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

        // Nothing is pending any more, so whatever was typed is a change worth repeating — unless
        // it changed nothing, in which case `.` should still put back whatever it put back last.
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
            // In insert mode the keymap only gets a say about the keys it has bound — `<Esc>` and
            // the like. Everything else is text, and the editor's own handling of it is better
            // than anything this could do: it knows about auto-indent and about grouping a whole
            // insertion into one undo step.
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

        // Not the engine's. The application knows what a picker or a language server is.
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

    // ---- Motions -----------------------------------------------------------------------------

    /// Where the caret is: the visual head in a visual mode, the primary caret otherwise.
    fn caret(&self, cx: &Context<'_>) -> usize {
        if self.mode.is_visual() {
            self.visual_head
        } else {
            cx.cursor()
        }
    }

    /// Where a motion goes, and what to do about having gone there.
    fn motion(&mut self, action: &Action, count: u32, cx: &Context<'_>) -> Step {
        let rope = cx.rope;
        let from = self.caret(cx);
        let args = &action.args;

        let target = match action.leaf() {
            "left" => motion::left(rope, from, count),
            "right" => motion::right(rope, from, count),
            "down" => motion::down(rope, from, count, self.goal_column),
            "up" => motion::up(rope, from, count, self.goal_column),
            "word_forward" => motion::word_forward(rope, from, count, args.flag("big")),
            "word_backward" => motion::word_backward(rope, from, count, args.flag("big")),
            "word_end" if args.flag("backward") => {
                motion::word_end_backward(rope, from, count, args.flag("big"))
            }
            "word_end" => motion::word_end(rope, from, count, args.flag("big")),
            "line_start" => motion::line_start(rope, from),
            "first_non_blank" => motion::first_non_blank(rope, from),
            "line_end" => motion::line_end(rope, from, count),
            "last_non_blank" => motion::last_non_blank(rope, from, count),
            "document_start" => motion::goto_line(
                rope,
                self.count.take().or(Some(count)).filter(|_| count > 1),
            ),
            "document_end" => motion::document_end(rope, (count > 1).then_some(count)),
            "paragraph_forward" => motion::paragraph_forward(rope, from, count),
            "paragraph_backward" => motion::paragraph_backward(rope, from, count),
            "matching_bracket" => match motion::matching_bracket(rope, from) {
                Some(target) => target,
                None => return Step::nothing(),
            },
            "screen_top" => motion::screen_top(rope, cx.view, count),
            "screen_middle" => motion::screen_middle(rope, cx.view),
            "screen_bottom" => motion::screen_bottom(rope, cx.view, count),
            "half_page_down" => motion::half_page_down(rope, from, cx.view, count),
            "half_page_up" => motion::half_page_up(rope, from, cx.view, count),
            "page_down" => motion::page_down(rope, from, cx.view, count),
            "page_up" => motion::page_up(rope, from, cx.view, count),
            "find_char" => {
                self.awaiting = Some(Awaiting::FindChar {
                    backward: args.flag("backward"),
                    till: args.flag("till"),
                    count,
                });
                return Step::Pending;
            }
            "repeat_find" => {
                let Some(find) = self.last_find else {
                    return Step::nothing();
                };
                let find = if args.flag("reverse") {
                    find.reversed()
                } else {
                    find
                };
                match motion::find_char(rope, from, count, find, true) {
                    Some(target) => target,
                    None => return Step::nothing(),
                }
            }
            _ => return Step::one(Effect::Complain(format!("no motion {}", action.name))),
        };

        // Only the two vertical motions keep a goal column; everything else sets a new one.
        if !matches!(action.leaf(), "down" | "up") {
            self.goal_column = None;
        } else if self.goal_column.is_none() {
            self.goal_column = Some(motion::column_of(rope, from));
        }

        self.go(target, cx)
    }

    /// Moves the caret to `target`, or hands the range to a waiting operator.
    fn go(&mut self, target: Target, cx: &Context<'_>) -> Step {
        if let Some(pending) = self.operator.take() {
            let range = operator_range(cx.rope, cx.cursor(), target);
            return self.apply_operator(&pending.action, range, target.kind == Kind::Linewise, cx);
        }

        if target.jump {
            self.jumps.push(self.caret(cx));
        }

        let byte = text::clamp_normal(cx.rope, target.byte);
        if self.mode.is_visual() {
            self.visual_head = byte;
        }

        Step::Consumed(vec![
            Effect::Select(self.selections_for(byte, cx)),
            Effect::Scroll(Scroll::EnsureVisible),
        ])
    }

    /// What a visual selection covers, and whether it is by lines.
    ///
    /// Charwise is inclusive of the character the caret is on, which is the whole difference
    /// between `vld` taking two characters and taking one.
    fn visual_ranges(&self, cx: &Context<'_>) -> (Vec<std::ops::Range<usize>>, bool) {
        let rope = cx.rope;
        let (anchor, head) = (self.visual_anchor, self.visual_head);
        match self.mode {
            Mode::VisualLine => {
                let (from, to) = (text::line_of(rope, anchor), text::line_of(rope, head));
                (vec![text::linewise_range(rope, from, to)], true)
            }
            Mode::VisualBlock => (
                block_selections(rope, anchor, head)
                    .into_iter()
                    .map(Selection::range)
                    .filter(|range| !range.is_empty())
                    .collect(),
                false,
            ),
            _ => {
                let (start, end) = (anchor.min(head), anchor.max(head));
                (
                    std::iter::once(start..text::next_grapheme(rope, end)).collect(),
                    false,
                )
            }
        }
    }

    /// The selections after the caret moves to `byte`.
    fn selections_for(&self, byte: usize, cx: &Context<'_>) -> Vec<Selection> {
        match self.mode {
            Mode::Visual | Mode::Select => vec![Selection::new(self.visual_anchor, byte)],
            Mode::VisualLine => {
                let rope = cx.rope;
                let (from, to) = (
                    text::line_of(rope, self.visual_anchor),
                    text::line_of(rope, byte),
                );
                let range = text::linewise_range(rope, from, to);
                // The head end is the one the caret is on, so `o` and further motion work.
                if byte >= self.visual_anchor {
                    vec![Selection::new(range.start, range.end)]
                } else {
                    vec![Selection::new(range.end, range.start)]
                }
            }
            Mode::VisualBlock => block_selections(cx.rope, self.visual_anchor, byte),
            _ => vec![Selection::caret(byte)],
        }
    }

    // ---- Text objects ------------------------------------------------------------------------

    /// What a text object selects, and what to do with it.
    fn text_object(&mut self, action: &Action, _count: u32, cx: &Context<'_>) -> Step {
        let rope = cx.rope;
        let at = cx.cursor();
        let args = &action.args;
        let around = args.flag("around");

        let range = match action.leaf() {
            "word" => textobject::word(rope, at, args.flag("big"), around),
            "paragraph" => textobject::paragraph(rope, at, around),
            "sentence" => textobject::sentence(rope, at, around),
            "quote" => args
                .char("quote")
                .and_then(|quote| textobject::quote(rope, at, quote, around)),
            "pair" => args
                .char("open")
                .and_then(|open| textobject::pair(rope, at, open, around)),
            // Tree-sitter's, which the application answers because it owns the parser.
            "function" | "class" => return Step::one(Effect::App(action.clone())),
            _ => return Step::one(Effect::Complain(format!("no text object {}", action.name))),
        };

        let Some(range) = range else {
            // Nothing to select. The operator stays pending, the way vim leaves it.
            return Step::nothing();
        };

        if let Some(pending) = self.operator.take() {
            return self.apply_operator(&pending.action, range, false, cx);
        }
        if self.mode.is_visual() {
            self.visual_anchor = range.start;
            self.visual_head = text::prev_grapheme(rope, range.end);
            return Step::one(Effect::Select(self.selections_for(self.visual_head, cx)));
        }
        Step::nothing()
    }

    // ---- Operators ---------------------------------------------------------------------------

    /// An operator: either applied to the selection, or waiting for what to apply to.
    fn operator(&mut self, action: &Action, count: u32, cx: &Context<'_>) -> Step {
        if self.mode.is_visual() {
            let (ranges, linewise) = self.visual_ranges(cx);
            self.mode = Mode::Normal;
            return self.apply_ranges(action, ranges, linewise, cx);
        }

        // Already pending and typed again means "this line", which the caller handles; reaching
        // here with one pending is a second, different operator, and the first is abandoned.
        self.operator = Some(PendingOperator {
            action: action.clone(),
            key: self.resolved,
            count,
        });
        self.mode = Mode::OperatorPending;
        Step::one(Effect::Mode(Mode::OperatorPending))
    }

    /// The operator, applied to whole lines from the caret.
    fn apply_linewise_operator(&mut self, action: &Action, count: u32, cx: &Context<'_>) -> Step {
        let rope = cx.rope;
        let line = text::line_of(rope, cx.cursor());
        let last = rope.len_lines().saturating_sub(1);
        let to = (line + count.max(1) as usize - 1).min(last);
        let range = text::linewise_range(rope, line, to);
        self.apply_operator(action, range, true, cx)
    }

    /// The operator, applied to one range.
    fn apply_operator(
        &mut self,
        action: &Action,
        range: std::ops::Range<usize>,
        linewise: bool,
        cx: &Context<'_>,
    ) -> Step {
        self.apply_ranges(action, vec![range], linewise, cx)
    }

    /// The operator, applied to every range it was given.
    fn apply_ranges(
        &mut self,
        action: &Action,
        ranges: Vec<std::ops::Range<usize>>,
        linewise: bool,
        cx: &Context<'_>,
    ) -> Step {
        let rope = cx.rope;
        let register = self.take_register();
        let mut effects = Vec::new();

        let ranges: Vec<std::ops::Range<usize>> = ranges
            .into_iter()
            .filter(|range| !range.is_empty() || linewise)
            .collect();

        if ranges.is_empty() {
            self.mode = Mode::Normal;
            return Step::one(Effect::Mode(Mode::Normal));
        }

        let taken: String = ranges
            .iter()
            .map(|range| rope.byte_slice(range.clone()).to_string())
            .collect::<Vec<_>>()
            .join("");

        let contents = if linewise {
            Contents::linewise(taken.clone())
        } else {
            Contents::charwise(taken.clone())
        };

        let first = ranges.first().expect("not empty").start;

        match action.leaf() {
            "yank" => {
                self.registers.yank(register, contents);
                if register.is_clipboard() {
                    effects.push(Effect::SetClipboard {
                        text: taken,
                        primary: register.character() == '*',
                    });
                }
                // The caret goes to the start of what was yanked, which is what vim does.
                effects.push(Effect::Select(vec![Selection::caret(text::clamp_normal(
                    rope, first,
                ))]));
                self.mode = Mode::Normal;
                effects.push(Effect::Mode(Mode::Normal));
            }
            "delete" | "change" => {
                self.registers.delete(register, contents);
                if register.is_clipboard() {
                    effects.push(Effect::SetClipboard {
                        text: taken,
                        primary: register.character() == '*',
                    });
                }
                let change = action.leaf() == "change";
                if change && linewise {
                    // `cc` empties the line and leaves the caret on it, rather than removing it.
                    let line = text::line_of(rope, first);
                    let indent = rope
                        .byte_slice(text::line_start(rope, line)..text::first_non_blank(rope, line))
                        .to_string();
                    effects.push(Effect::Replace(
                        ranges
                            .iter()
                            .map(|range| (range.clone(), indent.clone() + "\n"))
                            .collect(),
                    ));
                    effects.push(Effect::Select(vec![Selection::caret(first + indent.len())]));
                } else {
                    let caret = if linewise {
                        linewise_caret(rope, ranges.first().expect("not empty"))
                    } else {
                        first
                    };
                    effects.push(Effect::Replace(
                        ranges
                            .iter()
                            .map(|range| (range.clone(), String::new()))
                            .collect(),
                    ));
                    effects.push(Effect::Select(vec![Selection::caret(caret)]));
                }
                if change {
                    self.mode = Mode::Insert;
                    effects.push(Effect::Mode(Mode::Insert));
                } else {
                    self.mode = Mode::Normal;
                    effects.push(Effect::Mode(Mode::Normal));
                }
            }
            "indent" | "dedent" => {
                let dedent = action.leaf() == "dedent";
                let replacements = indent_lines(rope, &ranges, dedent);
                // Where the caret lands is measured against the text *after* the change: the
                // first line's own indent has just moved, and the caret goes to its first
                // non-blank.
                let line = text::line_of(rope, first);
                let moved: isize = replacements
                    .iter()
                    .filter(|(range, _)| text::line_of(rope, range.start) == line)
                    .map(|(range, text)| text.len() as isize - (range.end - range.start) as isize)
                    .sum();
                let caret = (text::first_non_blank(rope, line) as isize + moved).max(0) as usize;
                if !replacements.is_empty() {
                    effects.push(Effect::Replace(replacements));
                }
                effects.push(Effect::Select(vec![Selection::caret(caret)]));
                self.mode = Mode::Normal;
                effects.push(Effect::Mode(Mode::Normal));
            }
            "lowercase" | "uppercase" | "swap_case" => {
                let leaf = action.leaf();
                let replacements: Vec<_> = ranges
                    .iter()
                    .map(|range| {
                        let text = rope.byte_slice(range.clone()).to_string();
                        let changed = match leaf {
                            "lowercase" => text.to_lowercase(),
                            "uppercase" => text.to_uppercase(),
                            _ => swap_case(&text),
                        };
                        (range.clone(), changed)
                    })
                    .collect();
                effects.push(Effect::Replace(replacements));
                effects.push(Effect::Select(vec![Selection::caret(text::clamp_normal(
                    rope, first,
                ))]));
                self.mode = Mode::Normal;
                effects.push(Effect::Mode(Mode::Normal));
            }
            // Commenting needs to know the language's comment token, which the application owns.
            "comment" => {
                self.mode = Mode::Normal;
                effects.push(Effect::App(Action::with(
                    action.name.clone(),
                    action.args.clone(),
                )));
                effects.push(Effect::Mode(Mode::Normal));
            }
            other => {
                self.mode = Mode::Normal;
                effects.push(Effect::Complain(format!("no operator {other}")));
            }
        }

        effects.push(Effect::Scroll(Scroll::EnsureVisible));
        Step::Consumed(effects)
    }

    // ---- Modes -------------------------------------------------------------------------------

    /// Entering and leaving the modes.
    fn mode_change(&mut self, action: &Action, count: u32, cx: &Context<'_>) -> Step {
        let rope = cx.rope;
        let at = cx.cursor();

        match action.leaf() {
            "normal" => {
                let was = self.mode;
                self.operator = None;
                self.awaiting = None;
                self.keys.clear();
                self.count = None;
                self.register = None;
                self.mode = Mode::Normal;
                let mut effects = vec![Effect::Mode(Mode::Normal)];
                // Leaving insert puts the caret back onto a character, which is what makes the
                // block cursor land where vim leaves it.
                if was.is_inserting() || was.is_visual() {
                    let byte = if was.is_inserting() {
                        text::clamp_normal(
                            rope,
                            text::prev_grapheme(rope, at)
                                .max(text::line_start(rope, text::line_of(rope, at))),
                        )
                    } else {
                        text::clamp_normal(rope, at)
                    };
                    effects.push(Effect::Select(vec![Selection::caret(byte)]));
                }
                Step::Consumed(effects)
            }
            "insert" => {
                let byte = match action.args.str("at") {
                    Some("first_non_blank") => text::first_non_blank(rope, text::line_of(rope, at)),
                    Some("after") => text::next_grapheme(rope, at)
                        .min(text::line_end(rope, text::line_of(rope, at))),
                    Some("line_end") => text::line_end(rope, text::line_of(rope, at)),
                    _ => at,
                };
                self.mode = Mode::Insert;
                Step::Consumed(vec![
                    Effect::Select(vec![Selection::caret(byte)]),
                    Effect::Mode(Mode::Insert),
                ])
            }
            "replace" => {
                self.mode = Mode::Replace;
                Step::one(Effect::Mode(Mode::Replace))
            }
            "open_line" => {
                let above = action.args.flag("above");
                let line = text::line_of(rope, at);
                let indent = rope
                    .byte_slice(text::line_start(rope, line)..text::first_non_blank(rope, line))
                    .to_string();
                let (at, text) = if above {
                    let start = text::line_start(rope, line);
                    (start, format!("{indent}\n"))
                } else {
                    let end = text::line_end(rope, line);
                    (end, format!("\n{indent}"))
                };
                let caret = if above {
                    at + indent.len()
                } else {
                    at + 1 + indent.len()
                };
                self.mode = Mode::Insert;
                Step::Consumed(vec![
                    Effect::Replace(vec![(at..at, text)]),
                    Effect::Select(vec![Selection::caret(caret)]),
                    Effect::Mode(Mode::Insert),
                    Effect::Scroll(Scroll::EnsureVisible),
                ])
            }
            "visual" => {
                let kind = match action.args.str("kind") {
                    Some("line") => Mode::VisualLine,
                    Some("block") => Mode::VisualBlock,
                    _ => Mode::Visual,
                };
                if self.mode == kind {
                    // The same visual mode again leaves it, which is what vim does.
                    self.mode = Mode::Normal;
                    return Step::Consumed(vec![
                        Effect::Mode(Mode::Normal),
                        Effect::Select(vec![Selection::caret(text::clamp_normal(rope, at))]),
                    ]);
                }
                if !self.mode.is_visual() {
                    self.visual_anchor = at;
                }
                self.visual_head = at;
                self.mode = kind;
                let _ = count;
                Step::Consumed(vec![
                    Effect::Mode(kind),
                    Effect::Select(self.selections_for(at, cx)),
                ])
            }
            other => Step::one(Effect::Complain(format!("no mode {other}"))),
        }
    }

    /// What only makes sense with something selected.
    fn visual(&mut self, action: &Action, cx: &Context<'_>) -> Step {
        match action.leaf() {
            "swap_ends" => {
                std::mem::swap(&mut self.visual_anchor, &mut self.visual_head);
                let head = self.visual_head;
                Step::one(Effect::Select(self.selections_for(head, cx)))
            }
            // A block insert is several carets, which the editor's own multi-caret editing then
            // does the rest of.
            "block_insert" | "block_append" => {
                let append = action.leaf() == "block_append";
                let (ranges, _) = self.visual_ranges(cx);
                let carets: Vec<Selection> = ranges
                    .iter()
                    .map(|range| Selection::caret(if append { range.end } else { range.start }))
                    .collect();
                self.mode = Mode::Insert;
                Step::Consumed(vec![Effect::Select(carets), Effect::Mode(Mode::Insert)])
            }
            other => Step::one(Effect::Complain(format!("no visual command {other}"))),
        }
    }

    // ---- Edits -------------------------------------------------------------------------------

    /// The commands that change text without being an operator and a motion.
    fn edit(&mut self, action: &Action, count: u32, cx: &Context<'_>) -> Step {
        let rope = cx.rope;
        let at = cx.cursor();

        match action.leaf() {
            "undo" => Step::Consumed(vec![Effect::Undo, Effect::Scroll(Scroll::EnsureVisible)]),
            "redo" => Step::Consumed(vec![Effect::Redo, Effect::Scroll(Scroll::EnsureVisible)]),
            "delete_char" => {
                let register = self.take_register();
                let range = if action.args.flag("backward") {
                    let mut start = at;
                    for _ in 0..count {
                        start = text::prev_grapheme(rope, start);
                    }
                    start.max(text::line_start(rope, text::line_of(rope, at)))..at
                } else {
                    let mut end = at;
                    for _ in 0..count {
                        end = text::next_grapheme(rope, end);
                    }
                    at..end.min(text::line_end(rope, text::line_of(rope, at)))
                };
                if range.is_empty() {
                    return Step::nothing();
                }
                let taken = rope.byte_slice(range.clone()).to_string();
                self.registers.delete(register, Contents::charwise(taken));
                Step::Consumed(vec![
                    Effect::Replace(vec![(range.clone(), String::new())]),
                    Effect::Select(vec![Selection::caret(range.start)]),
                ])
            }
            "delete_to_line_end" | "change_to_line_end" => {
                let register = self.take_register();
                let end = text::line_end(rope, text::line_of(rope, at));
                let range = at..end;
                let taken = rope.byte_slice(range.clone()).to_string();
                self.registers.delete(register, Contents::charwise(taken));
                let change = action.leaf().starts_with("change");
                let mut effects = vec![
                    Effect::Replace(vec![(range, String::new())]),
                    Effect::Select(vec![Selection::caret(at)]),
                ];
                if change {
                    self.mode = Mode::Insert;
                    effects.push(Effect::Mode(Mode::Insert));
                }
                Step::Consumed(effects)
            }
            "yank_line" => {
                let register = self.take_register();
                let line = text::line_of(rope, at);
                let last = rope.len_lines().saturating_sub(1);
                let to = (line + count.max(1) as usize - 1).min(last);
                let range = text::linewise_range(rope, line, to);
                let taken = rope.byte_slice(range).to_string();
                self.registers.yank(register, Contents::linewise(taken));
                Step::nothing()
            }
            "replace_char" => {
                self.awaiting = Some(Awaiting::ReplaceChar { count });
                Step::Pending
            }
            "toggle_case_char" => {
                let mut end = at;
                for _ in 0..count {
                    end = text::next_grapheme(rope, end);
                }
                let end = end.min(text::line_end(rope, text::line_of(rope, at)));
                if end <= at {
                    return Step::nothing();
                }
                let text = rope.byte_slice(at..end).to_string();
                Step::Consumed(vec![
                    Effect::Replace(vec![(at..end, swap_case(&text))]),
                    Effect::Select(vec![Selection::caret(text::clamp_normal(rope, end))]),
                ])
            }
            "join_lines" => self.join(count, action.args.flag("keep_spaces"), cx),
            "paste" => self.paste(action, count, cx),
            "repeat" => self.repeat(),
            other => Step::one(Effect::Complain(format!("no edit {other}"))),
        }
    }

    /// `J`: the next line joined onto this one, `count` lines at a time.
    ///
    /// The break and the blanks that start the next line give way to one space — unless this line
    /// already ends in one, the next is empty, or this one is. Every join is computed against the
    /// text as it is now, which is what lets them all go in as one change.
    fn join(&mut self, count: u32, keep_spaces: bool, cx: &Context<'_>) -> Step {
        let rope = cx.rope;
        let line = text::line_of(rope, self.caret(cx));
        let last = rope.len_lines().saturating_sub(1);
        // `J` with no count joins two lines; with one, that many.
        let joins = count.max(2) as usize - 1;

        let mut replacements = Vec::new();
        let mut caret = self.caret(cx);
        let mut moved = 0isize;

        for step in 0..joins {
            let here = line + step;
            if here >= last {
                break;
            }
            let start = text::line_start(rope, here);
            let end = text::line_end(rope, here);
            let next_first = text::first_non_blank(rope, here + 1);
            let next_end = text::line_end(rope, here + 1);

            let (to, filler) = if keep_spaces {
                (text::line_start(rope, here + 1), String::new())
            } else {
                let ends_blank = end > start
                    && text::char_at(rope, text::prev_grapheme(rope, end))
                        .is_some_and(char::is_whitespace);
                let filler = if ends_blank || next_first == next_end || end == start {
                    String::new()
                } else {
                    " ".to_owned()
                };
                (next_first, filler)
            };

            // Where the caret ends up, in the text as it will be: on the join, which for several
            // joins is the last one.
            caret = (end as isize + moved) as usize;
            moved += filler.len() as isize - (to - end) as isize;
            replacements.push((end..to, filler));
        }

        if replacements.is_empty() {
            return Step::nothing();
        }
        Step::Consumed(vec![
            Effect::Replace(replacements),
            Effect::Select(vec![Selection::caret(caret)]),
        ])
    }

    /// `p` and `P`.
    fn paste(&mut self, action: &Action, count: u32, cx: &Context<'_>) -> Step {
        let rope = cx.rope;
        let at = cx.cursor();
        let before = action.args.flag("before");
        let register = self.take_register();

        if register.is_clipboard() {
            return Step::one(Effect::ReadClipboard {
                primary: register.character() == '*',
                before,
            });
        }

        let contents = self.registers.get(register);
        if contents.text.is_empty() {
            return Step::nothing();
        }
        let text = contents.text.repeat(count.max(1) as usize);

        if contents.linewise {
            let line = text::line_of(rope, at);
            let at = if before {
                text::line_start(rope, line)
            } else if line + 1 < rope.len_lines() {
                text::line_start(rope, line + 1)
            } else {
                // Below the last line: the break comes first instead of last.
                let end = rope.len_bytes();
                let text = format!("\n{}", text.trim_end_matches('\n'));
                let caret = end + 1;
                return Step::Consumed(vec![
                    Effect::Replace(vec![(end..end, text)]),
                    Effect::Select(vec![Selection::caret(caret)]),
                    Effect::Scroll(Scroll::EnsureVisible),
                ]);
            };
            // The caret lands on the first non-blank of what was pasted.
            let leading = text.len() - text.trim_start_matches([' ', '\t']).len();
            return Step::Consumed(vec![
                Effect::Replace(vec![(at..at, text)]),
                Effect::Select(vec![Selection::caret(at + leading)]),
                Effect::Scroll(Scroll::EnsureVisible),
            ]);
        }

        let at = if before {
            at
        } else {
            text::next_grapheme(rope, at)
                .min(text::line_end(rope, text::line_of(rope, at)))
                .max(at)
        };
        let caret = at + text.len();
        Step::Consumed(vec![
            Effect::Replace(vec![(at..at, text)]),
            // On the last character of what was pasted, which is where vim leaves it.
            Effect::Select(vec![Selection::caret(caret.saturating_sub(1).max(at))]),
            Effect::Scroll(Scroll::EnsureVisible),
        ])
    }

    /// `.`, which puts the last change back by replaying the keys that made it.
    fn repeat(&mut self) -> Step {
        match self.last_change.as_ref() {
            Some(repeat) => Step::one(Effect::App(Action::with(
                "vim.replay",
                Args::new(
                    [(
                        "keys".to_owned(),
                        toml::Value::String(crate::notation::format(&repeat.keys)),
                    )]
                    .into_iter()
                    .collect(),
                ),
            ))),
            None => Step::nothing(),
        }
    }

    // ---- The view ------------------------------------------------------------------------------

    /// Where the view goes, without the caret moving.
    fn scroll(&mut self, action: &Action, count: u32) -> Step {
        let scroll = match action.leaf() {
            "center" => Scroll::Center,
            "top" => Scroll::Top,
            "bottom" => Scroll::Bottom,
            "lines" => {
                let lines = action.args.number("lines").unwrap_or(1) as i32;
                Scroll::Lines(lines * count.max(1) as i32)
            }
            other => return Step::one(Effect::Complain(format!("no scroll {other}"))),
        };
        Step::one(Effect::Scroll(scroll))
    }

    // ---- Marks, jumps and macros -----------------------------------------------------------------

    /// `m` and the two ways of jumping to a mark.
    fn mark(&mut self, action: &Action) -> Step {
        match action.leaf() {
            "set" => {
                self.awaiting = Some(Awaiting::SetMark);
                Step::Pending
            }
            "jump" => {
                self.awaiting = Some(Awaiting::JumpMark {
                    line: action.args.flag("line"),
                });
                Step::Pending
            }
            other => Step::one(Effect::Complain(format!("no mark command {other}"))),
        }
    }

    /// The jump list and the change list.
    fn jump(&mut self, action: &Action, cx: &Context<'_>) -> Step {
        let target = match action.leaf() {
            "back" => self.jumps.back(cx.cursor()),
            "forward" => self.jumps.forward(),
            // The change list is the application's, because it is the editor that holds the
            // history the changes are in.
            "older_change" | "newer_change" => return Step::one(Effect::App(action.clone())),
            other => return Step::one(Effect::Complain(format!("no jump {other}"))),
        };
        match target {
            Some(byte) => Step::Consumed(vec![
                Effect::Select(vec![Selection::caret(text::clamp_normal(cx.rope, byte))]),
                Effect::Scroll(Scroll::EnsureVisible),
            ]),
            None => Step::nothing(),
        }
    }

    /// `q` and `@`.
    fn macro_action(&mut self, action: &Action) -> Step {
        match action.leaf() {
            "record" => {
                if let Some((name, mut keys)) = self.recording_macro.take() {
                    // The `q` that stopped it is not part of it.
                    keys.pop();
                    self.registers
                        .set_quietly(name, Contents::charwise(crate::notation::format(&keys)));
                    return Step::one(Effect::Say(format!("recorded @{name}")));
                }
                self.awaiting = Some(Awaiting::RecordMacro);
                Step::Pending
            }
            "play" => {
                self.awaiting = Some(Awaiting::PlayMacro);
                Step::Pending
            }
            other => Step::one(Effect::Complain(format!("no macro command {other}"))),
        }
    }

    // ---- Raw keys ------------------------------------------------------------------------------

    /// A key the engine asked for by itself, which no keymap gets a say in.
    fn literal(&mut self, waiting: Awaiting, chord: Chord, cx: &Context<'_>) -> Step {
        // Escape backs out of anything that was waiting for a key.
        if chord == Chord::named(Named::Escape) {
            self.operator = None;
            self.mode = if self.mode == Mode::OperatorPending {
                Mode::Normal
            } else {
                self.mode
            };
            return Step::one(Effect::Mode(self.mode));
        }

        match waiting {
            Awaiting::FindChar {
                backward,
                till,
                count,
            } => {
                let Some(character) = chord.inserted() else {
                    return Step::nothing();
                };
                let find = FindChar {
                    character,
                    backward,
                    till,
                };
                self.last_find = Some(find);
                match motion::find_char(cx.rope, cx.cursor(), count, find, false) {
                    Some(target) => self.go(target, cx),
                    None => {
                        self.operator = None;
                        Step::nothing()
                    }
                }
            }
            Awaiting::ReplaceChar { count } => {
                let Some(character) = chord.inserted() else {
                    return Step::nothing();
                };
                let rope = cx.rope;
                let at = cx.cursor();
                let mut end = at;
                for _ in 0..count {
                    end = text::next_grapheme(rope, end);
                }
                let end = end.min(text::line_end(rope, text::line_of(rope, at)));
                if end <= at {
                    return Step::nothing();
                }
                let replacement: String =
                    std::iter::repeat_n(character, count.max(1) as usize).collect();
                Step::Consumed(vec![
                    Effect::Replace(vec![(at..end, replacement)]),
                    Effect::Select(vec![Selection::caret(at)]),
                ])
            }
            Awaiting::SetMark => {
                let Some(character) = chord.inserted() else {
                    return Step::nothing();
                };
                self.marks.insert(character, cx.cursor());
                Step::nothing()
            }
            Awaiting::JumpMark { line } => {
                let Some(character) = chord.inserted() else {
                    return Step::nothing();
                };
                let Some(byte) = self.marks.get(&character).copied() else {
                    return Step::one(Effect::Complain(format!("no mark {character}")));
                };
                let byte = byte.min(cx.rope.len_bytes());
                let byte = if line {
                    text::first_non_blank(cx.rope, text::line_of(cx.rope, byte))
                } else {
                    byte
                };
                self.jumps.push(cx.cursor());
                Step::Consumed(vec![
                    Effect::Select(vec![Selection::caret(text::clamp_normal(cx.rope, byte))]),
                    Effect::Scroll(Scroll::EnsureVisible),
                ])
            }
            Awaiting::Register => {
                let Some(character) = chord.inserted() else {
                    return Step::nothing();
                };
                match Name::of(character) {
                    Some(name) => {
                        self.register = Some(name);
                        Step::Pending
                    }
                    None => Step::one(Effect::Complain(format!("no register {character}"))),
                }
            }
            Awaiting::RecordMacro => {
                let Some(character) = chord.inserted() else {
                    return Step::nothing();
                };
                if !character.is_ascii_alphanumeric() {
                    return Step::one(Effect::Complain(format!("cannot record into {character}")));
                }
                self.recording_macro = Some((character, Vec::new()));
                Step::one(Effect::Say(format!("recording @{character}")))
            }
            Awaiting::PlayMacro => {
                let Some(character) = chord.inserted() else {
                    return Step::nothing();
                };
                let name = if character == '@' {
                    match self.last_macro {
                        Some(name) => name,
                        None => return Step::nothing(),
                    }
                } else {
                    character
                };
                self.last_macro = Some(name);
                if self.macro_depth >= MACRO_DEPTH {
                    return Step::one(Effect::Complain("macros are nested too deeply".to_owned()));
                }
                let Some(name) = Name::of(name) else {
                    return Step::nothing();
                };
                let keys = self.registers.get(name).text;
                if keys.is_empty() {
                    return Step::nothing();
                }
                // The application replays them, because replaying needs the buffer as it is after
                // each key rather than as it was when the macro started.
                Step::one(Effect::App(Action::with(
                    "vim.replay",
                    Args::new(
                        [("keys".to_owned(), toml::Value::String(keys))]
                            .into_iter()
                            .collect(),
                    ),
                )))
            }
        }
    }

    /// Runs `keys` as though they had been typed, for `.` and for macros.
    ///
    /// The context is read again between keys by the caller, which is what makes a macro that
    /// edits and then moves work.
    pub fn begin_replay(&mut self) {
        self.replaying = true;
        self.macro_depth += 1;
    }

    /// Ends what [`begin_replay`](Self::begin_replay) started.
    pub fn end_replay(&mut self) {
        self.replaying = false;
        self.macro_depth = self.macro_depth.saturating_sub(1);
    }
}

/// Where the caret lands after whole lines are deleted.
///
/// On the first non-blank of the line that now sits where they were, or of the line above when
/// they were the last ones — which is what makes `dd` on the last line leave the caret on text
/// rather than at the end of the buffer.
fn linewise_caret(rope: &Rope, range: &std::ops::Range<usize>) -> usize {
    if range.end >= rope.len_bytes() {
        // The lines were last, so `range.start` is the break above them and the line it ends is
        // the one that becomes current. Nothing before the range moves.
        return text::first_non_blank(rope, text::line_of(rope, range.start));
    }
    // Everything after the range shifts back by its length.
    let following = text::first_non_blank(rope, text::line_of(rope, range.end));
    following - (range.end - range.start)
}

/// The bytes an operator takes, given where it started and where the motion went.
fn operator_range(rope: &Rope, from: usize, target: Target) -> std::ops::Range<usize> {
    match target.kind {
        Kind::Linewise => {
            let (one, two) = (text::line_of(rope, from), text::line_of(rope, target.byte));
            text::linewise_range(rope, one, two)
        }
        Kind::Exclusive => {
            if target.byte >= from {
                from..target.byte
            } else {
                target.byte..from
            }
        }
        Kind::Inclusive => {
            if target.byte >= from {
                from..text::next_grapheme(rope, target.byte)
            } else {
                target.byte..text::next_grapheme(rope, from)
            }
        }
    }
}

/// One level of indentation added to or taken off every line the ranges touch.
fn indent_lines(
    rope: &Rope,
    ranges: &[std::ops::Range<usize>],
    dedent: bool,
) -> Vec<(std::ops::Range<usize>, String)> {
    const INDENT: &str = "    ";

    let mut lines: Vec<usize> = Vec::new();
    for range in ranges {
        let from = text::line_of(rope, range.start);
        let end = range.end.saturating_sub(1).max(range.start);
        let to = text::line_of(rope, end);
        for line in from..=to {
            if !lines.contains(&line) {
                lines.push(line);
            }
        }
    }

    lines
        .into_iter()
        .filter_map(|line| {
            let start = text::line_start(rope, line);
            if dedent {
                let text = text::line_text(rope, line);
                let mut take = 0;
                for character in text.chars() {
                    if character == '\t' {
                        take += 1;
                        break;
                    }
                    if character == ' ' && take < INDENT.len() {
                        take += 1;
                    } else {
                        break;
                    }
                }
                (take > 0).then(|| (start..start + take, String::new()))
            } else if text::line_is_empty(rope, line) {
                // An empty line is left empty rather than filled with spaces.
                None
            } else {
                Some((start..start, INDENT.to_owned()))
            }
        })
        .collect()
}

/// Every letter's case turned over.
fn swap_case(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_uppercase() {
                character.to_lowercase().next().unwrap_or(character)
            } else if character.is_lowercase() {
                character.to_uppercase().next().unwrap_or(character)
            } else {
                character
            }
        })
        .collect()
}

/// The carets a block selection is, one per line.
fn block_selections(rope: &Rope, anchor: usize, head: usize) -> Vec<Selection> {
    let (one, two) = (
        motion::column_of(rope, anchor),
        motion::column_of(rope, head),
    );
    let (left, right) = (one.min(two), one.max(two));
    let (first, last) = {
        let (a, b) = (text::line_of(rope, anchor), text::line_of(rope, head));
        (a.min(b), a.max(b))
    };

    (first..=last)
        .map(|line| {
            let start = motion::byte_at_column(rope, line, left);
            let end = motion::byte_at_column(rope, line, right + 1);
            Selection::new(start, end.max(start))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{indent_lines, operator_range, swap_case};
    use crate::motion::Target;
    use ropey::Rope;

    #[test]
    fn an_exclusive_motion_leaves_the_byte_it_landed_on() {
        let rope = Rope::from_str("hello world");
        assert_eq!(operator_range(&rope, 0, Target::exclusive(6)), 0..6);
    }

    #[test]
    fn an_inclusive_motion_takes_it() {
        // The whole difference between `dw` and `de`.
        let rope = Rope::from_str("hello world");
        assert_eq!(operator_range(&rope, 0, Target::inclusive(4)), 0..5);
    }

    #[test]
    fn a_backward_motion_gives_the_same_range_the_other_way_round() {
        let rope = Rope::from_str("hello world");
        assert_eq!(operator_range(&rope, 6, Target::exclusive(0)), 0..6);
    }

    #[test]
    fn a_linewise_motion_takes_whole_lines() {
        let rope = Rope::from_str("one\ntwo\nthree\n");
        assert_eq!(operator_range(&rope, 0, Target::linewise(5)), 0..8);
    }

    #[test]
    fn indenting_leaves_an_empty_line_empty() {
        // Otherwise `>ap` would fill the blank lines with trailing spaces.
        let rope = Rope::from_str("one\n\ntwo\n");
        let whole = std::iter::once(0..9).collect::<Vec<_>>();
        let replacements = indent_lines(&rope, &whole, false);
        assert_eq!(replacements.len(), 2);
    }

    #[test]
    fn dedenting_takes_off_what_is_there_and_no_more() {
        let rope = Rope::from_str("  two spaces\n\tone tab\nnone\n");
        let whole = std::iter::once(0..26).collect::<Vec<_>>();
        let replacements = indent_lines(&rope, &whole, true);
        assert_eq!(
            replacements.len(),
            2,
            "the line with no indent is left alone"
        );
        assert_eq!(replacements[0].0, 0..2);
        assert_eq!(replacements[1].0.len(), 1, "one tab");
    }

    #[test]
    fn swapping_case_turns_every_letter_over() {
        assert_eq!(swap_case("Hello, World!"), "hELLO, wORLD!");
        assert_eq!(swap_case("123"), "123");
    }
}
