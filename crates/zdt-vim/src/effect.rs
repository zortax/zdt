//! What the engine asks for, and what it needs to know to decide.
//!
//! The engine never touches an editor. It is handed what the buffer looks like right now, and
//! answers a list of things to do. That is what lets a test drive the whole grammar by writing
//! down keys and asserting on text.

use std::ops::Range;

use crate::action::Action;
use crate::mode::Mode;
use crate::motion::View;

/// One caret, and what it has selected.
///
/// `anchor` is where the selection was started and `head` is where the caret is; head may be
/// before anchor, which is what selecting backwards is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Selection {
    /// Where the selection started.
    pub anchor: usize,
    /// Where the caret is.
    pub head: usize,
}

impl Selection {
    /// A caret with nothing selected.
    #[must_use]
    pub const fn caret(at: usize) -> Self {
        Self {
            anchor: at,
            head: at,
        }
    }

    /// A selection from `anchor` to `head`.
    #[must_use]
    pub const fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head }
    }

    /// The bytes it covers, whichever way round it is.
    #[must_use]
    pub fn range(self) -> Range<usize> {
        if self.anchor <= self.head {
            self.anchor..self.head
        } else {
            self.head..self.anchor
        }
    }

    /// The earlier of its two ends.
    #[must_use]
    pub fn start(self) -> usize {
        self.anchor.min(self.head)
    }

    /// The later of its two ends.
    #[must_use]
    pub fn end(self) -> usize {
        self.anchor.max(self.head)
    }

    /// Whether nothing is selected.
    #[must_use]
    pub fn is_caret(self) -> bool {
        self.anchor == self.head
    }
}

/// How a visual mode paints, which the selected bytes alone cannot say.
///
/// The caret is a cell rather than a byte: a linewise selection covers whole lines while its caret
/// stays on a column, and a block one reaches past the end of a short line. `lines` and `columns`
/// are the rectangle, and are empty in the two modes that are not a block — there the selected
/// bytes already describe what reads as selected.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Visual {
    /// The caret's line.
    pub line: usize,
    /// The caret's column, counted in graphemes.
    pub column: usize,
    /// The lines a block covers.
    pub lines: Range<usize>,
    /// The columns a block covers on each of them.
    pub columns: Range<usize>,
}

impl Visual {
    /// A caret at `line` and `column`, with no rectangle around it.
    #[must_use]
    pub fn at(line: usize, column: usize) -> Self {
        Self {
            line,
            column,
            lines: 0..0,
            columns: 0..0,
        }
    }
}

/// Where to move the view.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Scroll {
    /// Put the caret's line in the middle.
    Center,
    /// Put it at the top.
    Top,
    /// Put it at the bottom.
    Bottom,
    /// Move the view by lines, without moving the caret.
    Lines(i32),
    /// Bring the caret into view, which almost everything wants afterwards.
    EnsureVisible,
}

/// One thing for the application to do.
#[derive(Clone, PartialEq, Debug)]
pub enum Effect {
    /// Put the carets here. The first is the primary one.
    Select(Vec<Selection>),
    /// Paint a visual selection this way, or paint none at all.
    Visual(Option<Visual>),
    /// Light these ranges for a moment.
    ///
    /// What a command that leaves the text as it was answers with, so that the person can see
    /// what it took.
    Flash(Vec<Range<usize>>),
    /// Replace these ranges with this text, as one change.
    ///
    /// Ranges are in the text as it is now and must not overlap. Deleting is replacing with
    /// nothing, which is why there is no separate delete.
    Replace(Vec<(Range<usize>, String)>),
    /// Take the last change back.
    Undo,
    /// Put it back.
    Redo,
    /// Move the view.
    Scroll(Scroll),
    /// The mode is now this.
    Mode(Mode),
    /// Put this on a system clipboard.
    SetClipboard {
        /// The text.
        text: String,
        /// Whether it is the selection clipboard. The ordinary one otherwise.
        primary: bool,
    },
    /// Read a system clipboard, and paste what comes back.
    ReadClipboard {
        /// Whether it is the selection clipboard.
        primary: bool,
        /// Whether to put it before the caret. After it otherwise.
        before: bool,
    },
    /// Something the application owns: a picker, a language server, a window.
    App(Action),
    /// Show the buffer this place is in, and put the caret there.
    ///
    /// What a mark or a jump in another buffer answers with. The engine has never heard of a
    /// buffer, so it hands the number back and the application does the showing.
    GoTo(Place),
    /// Something to say in the status line.
    Say(String),
    /// Something that went wrong.
    Complain(String),
}

/// What the buffer looks like to the engine right now.
pub struct Context<'a> {
    /// The text.
    pub rope: &'a ropey::Rope,
    /// The carets, the first being the primary one. Never empty.
    pub selections: &'a [Selection],
    /// What the view is showing.
    pub view: View,
    /// Which buffer this is, as the application names them.
    ///
    /// Opaque here on purpose: the engine has never heard of a buffer and does not want to. It
    /// carries the number so that a mark and a jump can say *where* as well as *how far in*, and
    /// hands it back unchanged.
    pub owner: Owner,
}

/// Which buffer a place is in, as the application names them.
///
/// A number the engine never interprets. Zero is "the caller did not say", which is what a test
/// and an old session both mean.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Owner(pub u64);

/// A place in a buffer: which one, and how far in.
///
/// The buffer as well as the offset, because a bare offset from a previous run names a place in a
/// file something else may have edited, and `'a` landing three functions away is worse than `'a`
/// doing nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Place {
    /// Which buffer.
    pub owner: Owner,
    /// How far into it, in bytes.
    pub byte: usize,
}

impl Place {
    /// A place in `owner` at `byte`.
    #[must_use]
    pub fn new(owner: Owner, byte: usize) -> Self {
        Self { owner, byte }
    }
}

impl Context<'_> {
    /// Where the primary caret is, buffer and all.
    #[must_use]
    pub fn place(&self) -> Place {
        Place::new(self.owner, self.cursor())
    }

    /// The primary caret's head.
    #[must_use]
    pub fn cursor(&self) -> usize {
        self.selections
            .first()
            .map_or(0, |selection| selection.head)
    }

    /// The primary selection.
    #[must_use]
    pub fn primary(&self) -> Selection {
        self.selections
            .first()
            .copied()
            .unwrap_or(Selection::caret(0))
    }
}

/// What one key came to.
#[derive(Clone, PartialEq, Debug)]
pub enum Step {
    /// The key was used up. Here is what to do.
    Consumed(Vec<Effect>),
    /// The key is part of a sequence that has not finished.
    Pending,
    /// The key is not the engine's. The editor takes it, which makes typing in insert mode the
    /// editor's own business. That includes its auto-indent and its undo grouping.
    PassThrough,
}

impl Step {
    /// A step that did nothing, but used the key up.
    #[must_use]
    pub fn nothing() -> Self {
        Self::Consumed(Vec::new())
    }

    /// A step that does one thing.
    #[must_use]
    pub fn one(effect: Effect) -> Self {
        Self::Consumed(vec![effect])
    }

    /// The effects, when it was consumed.
    #[must_use]
    pub fn effects(&self) -> &[Effect] {
        match self {
            Self::Consumed(effects) => effects,
            Self::Pending | Self::PassThrough => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Effect, Selection, Step};

    #[test]
    fn a_selection_covers_the_same_bytes_either_way_round() {
        let forward = Selection::new(3, 7);
        let backward = Selection::new(7, 3);
        assert_eq!(forward.range(), backward.range());
        assert_eq!(forward.start(), 3);
        assert_eq!(forward.end(), 7);
        assert_eq!(backward.start(), 3);
    }

    #[test]
    fn a_caret_selects_nothing() {
        let caret = Selection::caret(4);
        assert!(caret.is_caret());
        assert!(caret.range().is_empty());
    }

    #[test]
    fn a_step_that_did_nothing_still_used_the_key() {
        // Which is the difference between a command that had no effect and a key the editor
        // should have been given.
        assert_eq!(Step::nothing(), Step::Consumed(Vec::new()));
        assert!(Step::nothing().effects().is_empty());
        assert!(Step::Pending.effects().is_empty());
        assert!(Step::PassThrough.effects().is_empty());
    }

    #[test]
    fn one_effect_is_a_step() {
        let step = Step::one(Effect::Undo);
        assert_eq!(step.effects(), &[Effect::Undo]);
    }
}
