//! Where a motion goes, and what an operator takes when it goes there.
//!
//! Every motion answers a byte and says how a range ending at it is measured. The three ways are
//! vim's, and getting them wrong is what makes an editor feel almost right:
//!
//! * **exclusive** — the byte itself is not taken. `dw` from the start of a word deletes the word
//!   and the space after it but not the letter it lands on.
//! * **inclusive** — the byte is taken. `de` deletes to the end of the word *including* the last
//!   letter, which is the whole difference between `de` and `dw`.
//! * **linewise** — whole lines, whatever the columns were. `dj` takes two entire lines.

use ropey::Rope;

use crate::text::{self, Class};

/// How the range between where a motion started and where it ended is measured.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// The byte the motion landed on is not part of the range.
    Exclusive,
    /// It is.
    Inclusive,
    /// Whole lines, whatever the columns were.
    Linewise,
}

/// Where a motion went.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Target {
    /// The byte it landed on.
    pub byte: usize,
    /// How a range ending there is measured.
    pub kind: Kind,
    /// Whether it is a jump, which is what the jump list remembers.
    pub jump: bool,
}

impl Target {
    /// An exclusive motion to `byte`.
    #[must_use]
    pub const fn exclusive(byte: usize) -> Self {
        Self {
            byte,
            kind: Kind::Exclusive,
            jump: false,
        }
    }

    /// An inclusive motion to `byte`.
    #[must_use]
    pub const fn inclusive(byte: usize) -> Self {
        Self {
            byte,
            kind: Kind::Inclusive,
            jump: false,
        }
    }

    /// A linewise motion to `byte`.
    #[must_use]
    pub const fn linewise(byte: usize) -> Self {
        Self {
            byte,
            kind: Kind::Linewise,
            jump: false,
        }
    }

    /// The same target, remembered by the jump list.
    #[must_use]
    pub const fn as_jump(mut self) -> Self {
        self.jump = true;
        self
    }
}

/// What the view is showing, which three motions need and nothing else does.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct View {
    /// The first line on screen.
    pub top_line: usize,
    /// How many whole lines it shows.
    pub height: usize,
}

impl View {
    /// The last line on screen.
    #[must_use]
    pub fn bottom_line(self) -> usize {
        self.top_line + self.height.saturating_sub(1)
    }
}

/// Left by `count` graphemes, stopping at the line's start.
///
/// Vim's `h` does not walk onto the line above, which is what makes it safe to hold down.
#[must_use]
pub fn left(rope: &Rope, from: usize, count: u32) -> Target {
    let start = text::line_start(rope, text::line_of(rope, from));
    let mut byte = from;
    for _ in 0..count.max(1) {
        if byte <= start {
            break;
        }
        byte = text::prev_grapheme(rope, byte);
    }
    Target::exclusive(byte.max(start))
}

/// Right by `count` graphemes, stopping at the line's end.
#[must_use]
pub fn right(rope: &Rope, from: usize, count: u32) -> Target {
    let end = text::line_end(rope, text::line_of(rope, from));
    let mut byte = from;
    for _ in 0..count.max(1) {
        if byte >= end {
            break;
        }
        byte = text::next_grapheme(rope, byte);
    }
    Target::exclusive(byte.min(end))
}

/// Down `count` lines, keeping the column when the line is long enough.
///
/// `goal` is the column the caret is aiming for, which is what makes a run of `j` through short
/// lines come back to where it started rather than sliding left.
#[must_use]
pub fn down(rope: &Rope, from: usize, count: u32, goal: Option<usize>) -> Target {
    vertical(rope, from, count.max(1) as isize, goal)
}

/// Up `count` lines, keeping the column.
#[must_use]
pub fn up(rope: &Rope, from: usize, count: u32, goal: Option<usize>) -> Target {
    vertical(rope, from, -(count.max(1) as isize), goal)
}

/// The column `byte` is in, counted in graphemes.
#[must_use]
pub fn column_of(rope: &Rope, byte: usize) -> usize {
    let line = text::line_of(rope, byte);
    let start = text::line_start(rope, line);
    let text = text::line_text(rope, line);
    let local = (byte - start).min(text.len());
    use unicode_segmentation::UnicodeSegmentation as _;
    text[..local].graphemes(true).count()
}

/// The byte at `column` graphemes into `line`, or the line's end.
#[must_use]
pub fn byte_at_column(rope: &Rope, line: usize, column: usize) -> usize {
    let start = text::line_start(rope, line);
    let text = text::line_text(rope, line);
    use unicode_segmentation::UnicodeSegmentation as _;
    match text.grapheme_indices(true).nth(column) {
        Some((offset, _)) => start + offset,
        None => start + text.len(),
    }
}

/// Up or down, keeping the column.
fn vertical(rope: &Rope, from: usize, lines: isize, goal: Option<usize>) -> Target {
    let line = text::line_of(rope, from);
    let column = goal.unwrap_or_else(|| column_of(rope, from));
    let last = rope.len_lines().saturating_sub(1);
    let target = (line as isize + lines).clamp(0, last as isize) as usize;
    Target::linewise(byte_at_column(rope, target, column))
}

/// The start of the next word, `count` times.
#[must_use]
pub fn word_forward(rope: &Rope, from: usize, count: u32, big: bool) -> Target {
    let mut byte = from;
    for _ in 0..count.max(1) {
        byte = next_word_start(rope, byte, big);
    }
    Target::exclusive(byte)
}

/// The start of the previous word, `count` times.
#[must_use]
pub fn word_backward(rope: &Rope, from: usize, count: u32, big: bool) -> Target {
    let mut byte = from;
    for _ in 0..count.max(1) {
        byte = previous_word_start(rope, byte, big);
    }
    Target::exclusive(byte)
}

/// The end of the current or next word, `count` times.
///
/// Inclusive, which is the whole difference between `de` and `dw`.
#[must_use]
pub fn word_end(rope: &Rope, from: usize, count: u32, big: bool) -> Target {
    let mut byte = from;
    for _ in 0..count.max(1) {
        byte = next_word_end(rope, byte, big);
    }
    Target::inclusive(byte)
}

/// The end of the previous word, `count` times.
#[must_use]
pub fn word_end_backward(rope: &Rope, from: usize, count: u32, big: bool) -> Target {
    let mut byte = from;
    for _ in 0..count.max(1) {
        byte = previous_word_end(rope, byte, big);
    }
    Target::inclusive(byte)
}

/// The start of the word after `from`.
fn next_word_start(rope: &Rope, from: usize, big: bool) -> usize {
    let length = rope.len_bytes();
    if from >= length {
        return length;
    }
    let start_class = text::class_at(rope, from, big);
    let mut byte = from;

    // Off the end of what we are on.
    if start_class != Class::Blank {
        while byte < length && text::class_at(rope, byte, big) == start_class {
            byte = text::next_grapheme(rope, byte);
        }
    }
    // Then over the blanks. An empty line is a word of its own, which is what makes `w` stop on
    // one rather than running past it.
    while byte < length && text::class_at(rope, byte, big) == Class::Blank {
        if text::line_is_empty(rope, text::line_of(rope, byte)) && byte != from {
            return byte;
        }
        byte = text::next_grapheme(rope, byte);
    }
    byte
}

/// The start of the word before `from`.
fn previous_word_start(rope: &Rope, from: usize, big: bool) -> usize {
    if from == 0 {
        return 0;
    }
    let mut byte = text::prev_grapheme(rope, from);

    while byte > 0 && text::class_at(rope, byte, big) == Class::Blank {
        if text::line_is_empty(rope, text::line_of(rope, byte)) {
            return byte;
        }
        byte = text::prev_grapheme(rope, byte);
    }

    let class = text::class_at(rope, byte, big);
    while byte > 0 {
        let previous = text::prev_grapheme(rope, byte);
        if text::class_at(rope, previous, big) != class {
            break;
        }
        byte = previous;
    }
    byte
}

/// The end of the word at or after `from`.
fn next_word_end(rope: &Rope, from: usize, big: bool) -> usize {
    let length = rope.len_bytes();
    if from >= length {
        return length;
    }
    let mut byte = text::next_grapheme(rope, from);

    while byte < length && text::class_at(rope, byte, big) == Class::Blank {
        byte = text::next_grapheme(rope, byte);
    }
    if byte >= length {
        return text::prev_grapheme(rope, length);
    }

    let class = text::class_at(rope, byte, big);
    loop {
        let next = text::next_grapheme(rope, byte);
        if next >= length || text::class_at(rope, next, big) != class {
            return byte;
        }
        byte = next;
    }
}

/// The end of the word before `from`.
fn previous_word_end(rope: &Rope, from: usize, big: bool) -> usize {
    if from == 0 {
        return 0;
    }
    let mut byte = text::prev_grapheme(rope, from);
    while byte > 0 && text::class_at(rope, byte, big) == Class::Blank {
        byte = text::prev_grapheme(rope, byte);
    }
    byte
}

/// The very start of the line.
#[must_use]
pub fn line_start(rope: &Rope, from: usize) -> Target {
    Target::exclusive(text::line_start(rope, text::line_of(rope, from)))
}

/// The first character on the line that is not a blank.
#[must_use]
pub fn first_non_blank(rope: &Rope, from: usize) -> Target {
    Target::exclusive(text::first_non_blank(rope, text::line_of(rope, from)))
}

/// The end of the line, `count - 1` lines down.
///
/// Inclusive, so `d$` takes the last character with it.
#[must_use]
pub fn line_end(rope: &Rope, from: usize, count: u32) -> Target {
    let line = text::line_of(rope, from) + count.max(1) as usize - 1;
    let line = line.min(rope.len_lines().saturating_sub(1));
    Target::inclusive(text::line_end(rope, line))
}

/// The last character on the line that is not a blank.
#[must_use]
pub fn last_non_blank(rope: &Rope, from: usize, count: u32) -> Target {
    let line = text::line_of(rope, from) + count.max(1) as usize - 1;
    let line = line.min(rope.len_lines().saturating_sub(1));
    Target::inclusive(text::last_non_blank(rope, line))
}

/// The first non-blank of `line`, counting from one, or of the first line.
#[must_use]
pub fn goto_line(rope: &Rope, line: Option<u32>) -> Target {
    let last = rope.len_lines().saturating_sub(1);
    let index = match line {
        Some(number) => (number.max(1) as usize - 1).min(last),
        None => 0,
    };
    Target::linewise(text::first_non_blank(rope, index)).as_jump()
}

/// The first non-blank of the last line, or of `line` when one was given.
#[must_use]
pub fn document_end(rope: &Rope, line: Option<u32>) -> Target {
    let last = rope.len_lines().saturating_sub(1);
    // A text ending in a break has an empty last line; `G` goes to the line with something on it.
    let last = if last > 0 && text::line_is_empty(rope, last) {
        last - 1
    } else {
        last
    };
    let index = match line {
        Some(number) => (number.max(1) as usize - 1).min(last),
        None => last,
    };
    Target::linewise(text::first_non_blank(rope, index)).as_jump()
}

/// The next empty line, `count` times.
#[must_use]
pub fn paragraph_forward(rope: &Rope, from: usize, count: u32) -> Target {
    let last = rope.len_lines().saturating_sub(1);
    let mut line = text::line_of(rope, from);
    for _ in 0..count.max(1) {
        line += 1;
        while line < last && !text::line_is_empty(rope, line) {
            line += 1;
        }
        // Over a run of empty lines, so a second `}` does not stop on the same gap.
        while line < last && text::line_is_empty(rope, line) && line == text::line_of(rope, from) {
            line += 1;
        }
        line = line.min(last);
    }
    Target::exclusive(text::line_start(rope, line.min(last)))
}

/// The previous empty line, `count` times.
#[must_use]
pub fn paragraph_backward(rope: &Rope, from: usize, count: u32) -> Target {
    let mut line = text::line_of(rope, from);
    for _ in 0..count.max(1) {
        if line == 0 {
            break;
        }
        line -= 1;
        while line > 0 && !text::line_is_empty(rope, line) {
            line -= 1;
        }
    }
    Target::exclusive(text::line_start(rope, line))
}

/// The bracket matching the one at or after the caret, on its line.
///
/// Nothing when there is no bracket on the rest of the line, which is what `%` does.
#[must_use]
pub fn matching_bracket(rope: &Rope, from: usize) -> Option<Target> {
    const PAIRS: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}')];

    let line = text::line_of(rope, from);
    let end = text::line_end(rope, line);

    // The first bracket at or after the caret, on this line.
    let mut byte = from;
    let (open, close, forward) = loop {
        if byte >= end {
            return None;
        }
        let character = text::char_at(rope, byte)?;
        if let Some((open, close)) = PAIRS.iter().find(|(open, _)| *open == character) {
            break (*open, *close, true);
        }
        if let Some((open, close)) = PAIRS.iter().find(|(_, close)| *close == character) {
            break (*open, *close, false);
        }
        byte = text::next_grapheme(rope, byte);
    };

    let length = rope.len_bytes();
    let mut depth = 0i32;
    let mut at = byte;
    loop {
        let character = text::char_at(rope, at);
        match character {
            Some(character) if character == open => depth += if forward { 1 } else { -1 },
            Some(character) if character == close => depth += if forward { -1 } else { 1 },
            _ => {}
        }
        if depth == 0 {
            return Some(Target::inclusive(at).as_jump());
        }
        if forward {
            let next = text::next_grapheme(rope, at);
            if next >= length {
                return None;
            }
            at = next;
        } else {
            if at == 0 {
                return None;
            }
            at = text::prev_grapheme(rope, at);
        }
    }
}

/// Where `f`, `F`, `t` and `T` go.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FindChar {
    /// What to look for.
    pub character: char,
    /// Whether to look backwards.
    pub backward: bool,
    /// Whether to stop before it rather than on it.
    pub till: bool,
}

impl FindChar {
    /// The same search, the other way round, which is what `,` does.
    #[must_use]
    pub const fn reversed(self) -> Self {
        Self {
            backward: !self.backward,
            ..self
        }
    }
}

/// The `count`th `find.character` on the caret's line, when there is one.
///
/// Inclusive going forwards and exclusive going backwards, which is vim's rule and is what makes
/// `dfx` take the `x` and `dFx` leave it.
///
/// `repeating` is what `;` and `,` pass. A `t` that has already stopped before a character would
/// find the same one again and stand still, so a repeat starts one further along — which is what
/// makes holding `;` walk down a line rather than stick.
#[must_use]
pub fn find_char(
    rope: &Rope,
    from: usize,
    count: u32,
    find: FindChar,
    repeating: bool,
) -> Option<Target> {
    let line = text::line_of(rope, from);
    let start = text::line_start(rope, line);
    let end = text::line_end(rope, line);

    let mut byte = from;
    if find.till && repeating {
        byte = if find.backward {
            if byte <= start {
                return None;
            }
            text::prev_grapheme(rope, byte)
        } else {
            if byte >= end {
                return None;
            }
            text::next_grapheme(rope, byte)
        };
    }

    for _ in 0..count.max(1) {
        byte = if find.backward {
            step_back_to(rope, byte, start, find.character)?
        } else {
            step_forward_to(rope, byte, end, find.character)?
        };
    }

    let byte = match (find.till, find.backward) {
        (true, false) => text::prev_grapheme(rope, byte),
        (true, true) => text::next_grapheme(rope, byte),
        (false, _) => byte,
    };

    Some(if find.backward {
        Target::exclusive(byte)
    } else {
        Target::inclusive(byte)
    })
}

/// The next `character` after `from`, on this side of `end`.
fn step_forward_to(rope: &Rope, from: usize, end: usize, character: char) -> Option<usize> {
    let mut byte = text::next_grapheme(rope, from);
    while byte < end {
        if text::char_at(rope, byte) == Some(character) {
            return Some(byte);
        }
        byte = text::next_grapheme(rope, byte);
    }
    None
}

/// The previous `character` before `from`, on this side of `start`.
fn step_back_to(rope: &Rope, from: usize, start: usize, character: char) -> Option<usize> {
    let mut byte = from;
    while byte > start {
        byte = text::prev_grapheme(rope, byte);
        if text::char_at(rope, byte) == Some(character) {
            return Some(byte);
        }
    }
    None
}

/// The first non-blank of the line `count` from the top of the view.
#[must_use]
pub fn screen_top(rope: &Rope, view: View, count: u32) -> Target {
    let line = (view.top_line + count.max(1) as usize - 1).min(rope.len_lines().saturating_sub(1));
    Target::linewise(text::first_non_blank(rope, line)).as_jump()
}

/// The first non-blank of the line in the middle of the view.
#[must_use]
pub fn screen_middle(rope: &Rope, view: View) -> Target {
    let last = rope.len_lines().saturating_sub(1);
    let bottom = view.bottom_line().min(last);
    let line = (view.top_line + bottom) / 2;
    Target::linewise(text::first_non_blank(rope, line.min(last))).as_jump()
}

/// The first non-blank of the line `count` from the bottom of the view.
#[must_use]
pub fn screen_bottom(rope: &Rope, view: View, count: u32) -> Target {
    let last = rope.len_lines().saturating_sub(1);
    let bottom = view.bottom_line().min(last);
    let line = bottom.saturating_sub(count.max(1) as usize - 1);
    Target::linewise(text::first_non_blank(
        rope,
        line.max(view.top_line).min(last),
    ))
    .as_jump()
}

/// Half a screen down, `count` times.
#[must_use]
pub fn half_page_down(rope: &Rope, from: usize, view: View, count: u32) -> Target {
    let lines = (view.height.max(2) / 2) * count.max(1) as usize;
    down(rope, from, lines as u32, None)
}

/// Half a screen up, `count` times.
#[must_use]
pub fn half_page_up(rope: &Rope, from: usize, view: View, count: u32) -> Target {
    let lines = (view.height.max(2) / 2) * count.max(1) as usize;
    up(rope, from, lines as u32, None)
}

/// A whole screen down, `count` times.
#[must_use]
pub fn page_down(rope: &Rope, from: usize, view: View, count: u32) -> Target {
    // Two lines of overlap, so the eye keeps its place — which is what vim does.
    let lines = view.height.saturating_sub(2).max(1) * count.max(1) as usize;
    down(rope, from, lines as u32, None)
}

/// A whole screen up, `count` times.
#[must_use]
pub fn page_up(rope: &Rope, from: usize, view: View, count: u32) -> Target {
    let lines = view.height.saturating_sub(2).max(1) * count.max(1) as usize;
    up(rope, from, lines as u32, None)
}

#[cfg(test)]
mod tests {
    use ropey::Rope;

    use super::{
        FindChar, Kind, View, byte_at_column, column_of, document_end, find_char, first_non_blank,
        goto_line, half_page_down, last_non_blank, left, line_end, line_start, matching_bracket,
        paragraph_backward, paragraph_forward, right, screen_bottom, screen_middle, screen_top,
        word_backward, word_end, word_end_backward, word_forward,
    };

    fn rope(text: &str) -> Rope {
        Rope::from_str(text)
    }

    #[test]
    fn h_and_l_stay_on_their_line() {
        // Which is what makes them safe to hold down.
        let rope = rope("ab\ncd");
        assert_eq!(left(&rope, 0, 1).byte, 0);
        assert_eq!(right(&rope, 2, 1).byte, 2);
        assert_eq!(right(&rope, 0, 1).byte, 1);
        assert_eq!(left(&rope, 2, 1).byte, 1);
    }

    #[test]
    fn a_count_moves_that_many_times() {
        let rope = rope("abcdef");
        assert_eq!(right(&rope, 0, 3).byte, 3);
        assert_eq!(left(&rope, 5, 2).byte, 3);
    }

    #[test]
    fn a_word_forward_lands_on_the_next_words_start() {
        let rope = rope("foo bar baz");
        assert_eq!(word_forward(&rope, 0, 1, false).byte, 4);
        assert_eq!(word_forward(&rope, 0, 2, false).byte, 8);
    }

    #[test]
    fn a_small_word_stops_at_punctuation_and_a_big_one_does_not() {
        let rope = rope("foo.bar baz");
        assert_eq!(word_forward(&rope, 0, 1, false).byte, 3, "the dot");
        assert_eq!(word_forward(&rope, 0, 1, true).byte, 8, "past it all");
    }

    #[test]
    fn a_word_backward_lands_on_the_previous_words_start() {
        let rope = rope("foo bar baz");
        assert_eq!(word_backward(&rope, 8, 1, false).byte, 4);
        assert_eq!(word_backward(&rope, 8, 2, false).byte, 0);
        assert_eq!(word_backward(&rope, 0, 1, false).byte, 0);
    }

    #[test]
    fn a_word_end_is_inclusive_which_is_what_makes_de_different_from_dw() {
        let rope = rope("foo bar");
        let target = word_end(&rope, 0, 1, false);
        assert_eq!(target.byte, 2, "the second `o`");
        assert_eq!(target.kind, Kind::Inclusive);
        assert_eq!(word_forward(&rope, 0, 1, false).kind, Kind::Exclusive);
    }

    #[test]
    fn a_word_end_from_the_end_of_a_word_goes_to_the_next_one() {
        let rope = rope("foo bar");
        assert_eq!(word_end(&rope, 2, 1, false).byte, 6);
    }

    #[test]
    fn ge_goes_back_to_the_previous_words_end() {
        let rope = rope("foo bar");
        assert_eq!(word_end_backward(&rope, 4, 1, false).byte, 2);
    }

    #[test]
    fn a_word_motion_crosses_lines() {
        let rope = rope("foo\nbar");
        assert_eq!(word_forward(&rope, 0, 1, false).byte, 4);
        assert_eq!(word_backward(&rope, 4, 1, false).byte, 0);
    }

    #[test]
    fn an_empty_line_is_a_word_of_its_own() {
        // Which is what makes `w` stop on a blank line rather than running past it.
        let rope = rope("foo\n\nbar");
        assert_eq!(word_forward(&rope, 0, 1, false).byte, 4);
        assert_eq!(word_forward(&rope, 4, 1, false).byte, 5);
    }

    #[test]
    fn the_ends_of_a_line_are_where_they_should_be() {
        let rope = rope("    let x = 1;\nnext");
        assert_eq!(line_start(&rope, 6).byte, 0);
        assert_eq!(first_non_blank(&rope, 6).byte, 4);
        let end = line_end(&rope, 0, 1);
        assert_eq!(end.byte, 14);
        assert_eq!(end.kind, Kind::Inclusive, "`d$` takes the last character");
        assert_eq!(last_non_blank(&rope, 0, 1).byte, 13);
    }

    #[test]
    fn a_count_on_dollar_reaches_a_later_lines_end() {
        let rope = rope("one\ntwo\nthree");
        assert_eq!(line_end(&rope, 0, 2).byte, 7);
    }

    #[test]
    fn going_to_a_line_lands_on_its_first_non_blank() {
        let rope = rope("one\n    two\nthree");
        assert_eq!(goto_line(&rope, Some(2)).byte, 8);
        assert!(goto_line(&rope, Some(2)).jump, "the jump list remembers it");
        assert_eq!(goto_line(&rope, None).byte, 0);
    }

    #[test]
    fn capital_g_goes_to_the_last_line_with_something_on_it() {
        // A text ending in a break has an empty line after it that `G` must not land on.
        let rope = rope("one\ntwo\n");
        assert_eq!(document_end(&rope, None).byte, 4);
        assert_eq!(document_end(&rope, Some(1)).byte, 0);
    }

    #[test]
    fn paragraphs_are_the_empty_lines_between_them() {
        let rope = rope("one\ntwo\n\nthree\nfour\n\nfive");
        assert_eq!(paragraph_forward(&rope, 0, 1).byte, 8);
        assert_eq!(paragraph_forward(&rope, 0, 2).byte, 20);
        assert_eq!(paragraph_backward(&rope, 22, 1).byte, 20);
    }

    #[test]
    fn a_bracket_matches_its_partner_either_way() {
        let rope = rope("fn main() { let x = (1 + 2); }");
        let forward = matching_bracket(&rope, 10).expect("the brace matches");
        assert_eq!(forward.byte, 29);
        let backward = matching_bracket(&rope, 29).expect("the brace matches");
        assert_eq!(backward.byte, 10);
        assert_eq!(forward.kind, Kind::Inclusive);
    }

    #[test]
    fn percent_finds_the_first_bracket_after_the_caret() {
        let rope = rope("let x = (1);");
        assert_eq!(
            matching_bracket(&rope, 0).expect("there is one").byte,
            10,
            "it looked forward for the `(` and matched it"
        );
    }

    #[test]
    fn percent_with_no_bracket_on_the_line_goes_nowhere() {
        let rope = rope("let x = 1;\n(nope)");
        assert_eq!(matching_bracket(&rope, 0), None);
    }

    #[test]
    fn a_nested_bracket_matches_the_right_one() {
        let rope = rope("((a))");
        assert_eq!(matching_bracket(&rope, 0).expect("matched").byte, 4);
        assert_eq!(matching_bracket(&rope, 1).expect("matched").byte, 3);
    }

    #[test]
    fn f_lands_on_the_character_and_t_stops_before_it() {
        let rope = rope("hello world");
        let find = |till, backward| FindChar {
            character: 'o',
            backward,
            till,
        };
        assert_eq!(
            find_char(&rope, 0, 1, find(false, false), false)
                .unwrap()
                .byte,
            4
        );
        assert_eq!(
            find_char(&rope, 0, 1, find(true, false), false)
                .unwrap()
                .byte,
            3
        );
        assert_eq!(
            find_char(&rope, 0, 2, find(false, false), false)
                .unwrap()
                .byte,
            7
        );
    }

    #[test]
    fn f_is_inclusive_and_capital_f_is_exclusive() {
        // Vim's rule: `dfx` takes the `x`, `dFx` leaves it.
        let rope = rope("hello world");
        let forward = find_char(
            &rope,
            0,
            1,
            FindChar {
                character: 'o',
                backward: false,
                till: false,
            },
            false,
        )
        .unwrap();
        let backward = find_char(
            &rope,
            7,
            1,
            FindChar {
                character: 'o',
                backward: true,
                till: false,
            },
            false,
        )
        .unwrap();
        assert_eq!(forward.kind, Kind::Inclusive);
        assert_eq!(backward.kind, Kind::Exclusive);
        assert_eq!(backward.byte, 4);
    }

    #[test]
    fn a_find_stops_at_the_end_of_the_line() {
        let rope = rope("abc\nxyz");
        assert_eq!(
            find_char(
                &rope,
                0,
                1,
                FindChar {
                    character: 'z',
                    backward: false,
                    till: false
                },
                false,
            ),
            None
        );
    }

    #[test]
    fn a_count_on_till_counts_the_characters_rather_than_the_steps() {
        let rope = rope("a.b.c");
        let till = FindChar {
            character: '.',
            backward: false,
            till: true,
        };
        // Already just before the first dot, so one of them is no movement.
        assert_eq!(find_char(&rope, 0, 1, till, false).unwrap().byte, 0);
        assert_eq!(find_char(&rope, 0, 2, till, false).unwrap().byte, 2);
    }

    #[test]
    fn a_repeated_till_makes_progress() {
        // `;` after a `t` must walk down the line rather than finding the same character again
        // and standing still.
        let rope = rope("a.b.c");
        let till = FindChar {
            character: '.',
            backward: false,
            till: true,
        };
        assert_eq!(find_char(&rope, 0, 1, till, true).unwrap().byte, 2);
        assert_eq!(find_char(&rope, 2, 1, till, true), None, "no third dot");
    }

    #[test]
    fn the_screen_motions_read_the_view() {
        let rope = rope("1\n2\n3\n4\n5\n6\n7\n8\n9\n10");
        let view = View {
            top_line: 2,
            height: 5,
        };
        assert_eq!(screen_top(&rope, view, 1).byte, 4, "line 3");
        assert_eq!(screen_bottom(&rope, view, 1).byte, 12, "line 7");
        assert_eq!(screen_middle(&rope, view).byte, 8, "line 5");
        assert_eq!(screen_top(&rope, view, 2).byte, 6, "line 4");
    }

    #[test]
    fn a_column_survives_a_short_line_in_between() {
        // The whole reason a goal column exists: `j` through a short line and back must land
        // where it started.
        let rope = rope("longer line\nab\nanother long one");
        let goal = column_of(&rope, 8);
        let short = super::down(&rope, 8, 1, Some(goal));
        assert_eq!(short.byte, 14, "the short line's end");
        let back = super::down(&rope, short.byte, 1, Some(goal));
        assert_eq!(column_of(&rope, back.byte), 8);
    }

    #[test]
    fn vertical_motions_are_linewise() {
        let rope = rope("one\ntwo\nthree");
        assert_eq!(super::down(&rope, 0, 1, None).kind, Kind::Linewise);
        assert_eq!(super::up(&rope, 4, 1, None).kind, Kind::Linewise);
    }

    #[test]
    fn vertical_motions_stop_at_the_ends_of_the_text() {
        let rope = rope("one\ntwo");
        assert_eq!(super::up(&rope, 0, 5, None).byte, 0);
        assert_eq!(super::down(&rope, 0, 99, None).byte, 4);
    }

    #[test]
    fn a_column_is_counted_in_graphemes() {
        let rope = rope("a\u{1F1E9}\u{1F1EA}b");
        assert_eq!(column_of(&rope, 9), 2);
        assert_eq!(byte_at_column(&rope, 0, 2), 9);
        assert_eq!(byte_at_column(&rope, 0, 99), 10, "the line's end");
    }

    #[test]
    fn half_a_page_is_half_the_view() {
        let rope = rope("1\n2\n3\n4\n5\n6\n7\n8\n9\n10");
        let view = View {
            top_line: 0,
            height: 6,
        };
        assert_eq!(
            half_page_down(&rope, 0, view, 1).byte,
            6,
            "three lines down"
        );
    }
}
