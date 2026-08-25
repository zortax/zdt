//! What a terminal holds, as the vim engine reads it.
//!
//! The engine has never heard of a terminal. It is handed a rope, some carets and a view, and
//! answers a list of things to do — the same as over a file. This is the seam for the other kind
//! of surface: the text is every line the terminal holds, and what comes back is drawn on the
//! grid rather than in an editor.
//!
//! # Why the text is a projection
//!
//! The emulator's grid is the truth and stays that way. The rope here is built from it and thrown
//! away when it moves, so there is one place a line lives and one place a caret does. Building it
//! costs the size of the history, so it is built when the terminal reports that something was
//! drawn and kept until it reports again — at most once per key.
//!
//! # What it refuses
//!
//! Everything that would change what is on the screen. A terminal takes text and never gives any
//! back: an insertion is sent to the program, and anything that would remove what is there is
//! said out loud instead.

pub mod map;

use std::cell::RefCell;
use std::rc::Rc;

use ropey::Rope;
use zdt_vim::effect::{Context, Owner, Scroll, Selection, Visual};
use zdt_vim::motion::View;
use zgui::reactive::prelude::*;
use zgui_terminal::{AppCursor, GridPoint, GridSpan, SpanKind, TerminalHandle};

use crate::workspace::BufferId;

/// One terminal's contents, as the vim engine reads them.
///
/// Cloning one is cloning a handle: every clone is the same caret over the same terminal.
#[derive(Clone)]
pub struct Scrollback {
    inner: Rc<Inner>,
}

struct Inner {
    /// Which terminal this is, so a mark says which buffer it is in.
    buffer: BufferId,
    handle: TerminalHandle,
    /// The lines, and what they were read at.
    text: RefCell<Projection>,
    /// The carets, the first being the primary one. Never empty.
    selections: RefCell<Vec<Selection>>,
    /// Which cell the cursor is drawn on.
    ///
    /// Held apart from the carets, because in a visual mode they are not the same thing. A
    /// charwise selection reaches one character past the caret so that `vy` takes the character
    /// it is on; a linewise one covers whole lines and ends at the start of the next; a block is
    /// one caret per line and its own reaches past the end of a short one, where there is no byte
    /// to be at. The engine names the caret itself, and that is what is drawn.
    caret: std::cell::Cell<GridPoint>,
}

/// The lines the engine reads, and how fresh they are.
struct Projection {
    rope: Rope,
    /// Which line the screen starts at, which is how many the history holds.
    screen: usize,
    /// What the terminal's revision was when this was built.
    revision: u64,
}

impl Scrollback {
    /// Reads what `handle` holds, with the caret where the program's cursor is.
    #[must_use]
    pub fn new(buffer: BufferId, handle: TerminalHandle) -> Self {
        let contents = handle.contents();
        let projection = Projection {
            rope: rope_of(&contents.lines),
            screen: contents.screen,
            revision: handle.revision().get_untracked(),
        };
        let at = map::byte_of(&projection.rope, contents.cursor);
        Self {
            inner: Rc::new(Inner {
                buffer,
                handle,
                text: RefCell::new(projection),
                selections: RefCell::new(vec![Selection::caret(at)]),
                caret: std::cell::Cell::new(contents.cursor),
            }),
        }
    }

    /// Which terminal this is.
    #[must_use]
    pub fn buffer(&self) -> BufferId {
        self.inner.buffer
    }

    // ---- What the engine reads -------------------------------------------------------------

    /// Reads what the engine needs to decide.
    ///
    /// The lines are read again first when the terminal has drawn anything since the last time.
    pub fn query<R>(&self, owner: Owner, read: impl FnOnce(&Context<'_>) -> R) -> R {
        self.refresh();
        let text = self.inner.text.borrow();
        let selections = self.inner.selections.borrow();
        let view = self.view(&text);
        read(&Context {
            rope: &text.rope,
            selections: &selections,
            view,
            owner,
        })
    }

    /// What the screen is showing, in lines of the projection.
    fn view(&self, text: &Projection) -> View {
        let scroll = self.inner.handle.scroll_state().get_untracked();
        View {
            top_line: text.screen.saturating_sub(scroll.display_offset),
            height: scroll.screen_lines.max(1),
        }
    }

    /// Reads the lines again when the terminal has drawn since they were last read.
    ///
    /// The carets keep the line and column they were on, and the cursor is already a cell so it
    /// keeps its place by itself. A history that is full drops its oldest line to make room for a
    /// new one, and a caret in one of those lines moves with the text under it; everywhere else
    /// the line it was on is the line it stays on.
    fn refresh(&self) {
        let revision = self.inner.handle.revision().get_untracked();
        if self.inner.text.borrow().revision == revision {
            return;
        }
        let contents = self.inner.handle.contents();
        let places: Vec<(GridPoint, GridPoint)> = {
            let text = self.inner.text.borrow();
            self.inner
                .selections
                .borrow()
                .iter()
                .map(|one| {
                    (
                        map::point_of(&text.rope, one.anchor),
                        map::point_of(&text.rope, one.head),
                    )
                })
                .collect()
        };

        let rope = rope_of(&contents.lines);
        let moved: Vec<Selection> = places
            .into_iter()
            .map(|(anchor, head)| {
                Selection::new(map::byte_of(&rope, anchor), map::byte_of(&rope, head))
            })
            .collect();
        *self.inner.text.borrow_mut() = Projection {
            rope,
            screen: contents.screen,
            revision,
        };
        *self.inner.selections.borrow_mut() = moved;
    }

    // ---- What the engine asks for ------------------------------------------------------------

    /// Puts the carets here, and the cursor on the first of them.
    ///
    /// Right in every mode that has no selection. A visual mode says where its caret is straight
    /// afterwards, through [`Self::paint`], and that is what ends up drawn.
    pub fn select(&self, selections: &[Selection]) {
        let Some(primary) = selections.first() else {
            return;
        };
        self.inner
            .caret
            .set(map::point_of(&self.inner.text.borrow().rope, primary.head));
        *self.inner.selections.borrow_mut() = selections.to_vec();
        self.show_cursor();
    }

    /// Draws the cursor where the caret is, and brings it into view.
    pub fn show_cursor(&self) {
        let at = self.inner.caret.get();
        self.inner.handle.set_cursor(Some(AppCursor::block(at)));
        self.inner.handle.reveal(at.line);
    }

    /// Takes the cursor away, leaving the program's, and everything drawn with it.
    pub fn hide_cursor(&self) {
        self.inner.handle.set_cursor(None);
        self.inner.handle.clear_selection();
        self.unflash();
    }

    /// Paints what a visual mode has selected as `kind`, and puts the cursor where its caret is.
    ///
    /// A block is drawn from the rectangle the engine reports rather than from the carets, so it
    /// covers the blank cells past the end of a short line the way it does in a text buffer. What
    /// a yank takes is still the carets, which stop at each line's end — the cells are selected,
    /// and the text that is there is what comes out.
    ///
    /// Everything else is drawn from the carets, whose bytes already say which cells read as
    /// selected. The caret is a line and a column in both: the selected bytes reach past it, and
    /// a block's own has no byte to be at once it is out among the blanks.
    pub fn paint(&self, visual: &Visual, kind: SpanKind) {
        let span = {
            let text = self.inner.text.borrow();
            let cell = |column| map::cell_of(&text.rope, visual.line, column);
            self.inner
                .caret
                .set(GridPoint::new(visual.line, cell(visual.column)));

            if kind == SpanKind::Block {
                Some(GridSpan {
                    from: GridPoint::new(visual.lines.start, cell(visual.columns.start)),
                    to: GridPoint::new(
                        visual.lines.end.saturating_sub(1),
                        cell(visual.columns.end.saturating_sub(1)),
                    ),
                    kind,
                })
            } else {
                let selections = self.inner.selections.borrow();
                selections
                    .first()
                    .zip(selections.last())
                    .map(|(first, last)| GridSpan {
                        from: map::point_of(&text.rope, first.start()),
                        // The selected bytes end past the last character, and a span ends on it.
                        to: map::point_of(&text.rope, before(&text.rope, last.end())),
                        kind,
                    })
            }
        };
        match span {
            Some(span) => self.inner.handle.select(span),
            None => self.inner.handle.clear_selection(),
        }
        self.show_cursor();
    }

    /// Takes the painting off, leaving the cursor where it is.
    ///
    /// Nothing is selected in a mode nobody is in.
    pub fn unpaint(&self) {
        self.inner.handle.clear_selection();
    }

    /// Lights `ranges` on the grid, so that what a command took can be seen.
    ///
    /// Answers whether anything was lit. One span per range, so a block yank lights the rectangle
    /// it took rather than everything between its corners. A light is its own layer, so the mode
    /// change that follows a yank does not take it off again.
    pub fn flash(&self, ranges: &[std::ops::Range<usize>]) -> bool {
        let text = self.inner.text.borrow();
        let spans: Vec<GridSpan> = ranges
            .iter()
            .filter(|range| !range.is_empty())
            .map(|range| {
                GridSpan::cells(
                    map::point_of(&text.rope, range.start),
                    // A range ends past the last character, and a span ends on it.
                    map::point_of(&text.rope, before(&text.rope, range.end)),
                )
            })
            .collect();
        if spans.is_empty() {
            return false;
        }
        self.inner.handle.flash(spans);
        true
    }

    /// Puts the light out.
    pub fn unflash(&self) {
        self.inner.handle.flash(Vec::new());
    }

    /// Moves the view.
    pub fn scroll(&self, scroll: Scroll) {
        use zgui_terminal::ScrollRequest;

        let handle = &self.inner.handle;
        match scroll {
            Scroll::Lines(lines) => handle.scroll(ScrollRequest::Lines(-lines)),
            // The caret is what a terminal follows, and it is already where it should be. There
            // is no room above the newest line to put it in the middle of, so the three that ask
            // for a place on the screen come to the same as bringing it into view.
            Scroll::Center | Scroll::Top | Scroll::Bottom | Scroll::EnsureVisible => {
                let text = self.inner.text.borrow();
                let head = self
                    .inner
                    .selections
                    .borrow()
                    .first()
                    .map_or(0, |one| one.head);
                handle.reveal(map::point_of(&text.rope, head).line);
            }
        }
    }

    /// Sends `text` to the program, as pasting it would.
    pub fn insert(&self, text: &str) {
        if !text.is_empty() {
            self.inner.handle.paste(text.to_owned());
        }
    }
}

/// Every line, as one text.
fn rope_of(lines: &[String]) -> Rope {
    let mut text = String::with_capacity(lines.iter().map(|line| line.len() + 1).sum());
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    Rope::from(text)
}

/// The byte before `byte`, which is where a range that ends past a character sits on it.
fn before(rope: &Rope, byte: usize) -> usize {
    let byte = byte.min(rope.len_bytes());
    let character = rope.byte_to_char(byte);
    rope.char_to_byte(character.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::{before, rope_of};

    #[test]
    fn every_line_is_one_line() {
        let rope = rope_of(&["one".to_owned(), String::new(), "two".to_owned()]);
        assert_eq!(rope.to_string(), "one\n\ntwo\n");
        assert_eq!(rope.len_lines(), 4);
    }

    #[test]
    fn the_byte_before_the_end_of_a_range_is_its_last_character() {
        let rope = ropey::Rope::from_str("ab\n");
        assert_eq!(before(&rope, 2), 1);
        assert_eq!(before(&rope, 0), 0);
    }
}
