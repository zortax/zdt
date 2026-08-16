//! Where a motion goes, and what an operator takes when it goes there.
//!
//! Every motion answers a byte and says how a range ending at it is measured. The three ways are
//! vim's. Getting them wrong is what makes an editor feel almost right:
//!
//! * **exclusive**: the byte itself stays. `dw` from the start of a word deletes the word and the
//!   space after it, and leaves the letter it lands on.
//! * **inclusive**: the byte goes. `de` deletes to the end of the word *including* the last
//!   letter, which is the whole difference between `de` and `dw`.
//! * **linewise**: whole lines, whatever the columns were. `dj` takes two entire lines.

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
/// `goal` is the column the caret is aiming for. That is what brings a run of `j` through short
/// lines back to the column it started in.
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
    // Then over the blanks. An empty line is a word of its own, so `w` stops on one.
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
    /// Whether to stop before it. On it otherwise.
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
/// Inclusive going forwards and exclusive going backwards. That is vim's rule, and it is what
/// makes `dfx` take the `x` and `dFx` leave it.
///
/// `repeating` is what `;` and `,` pass. A `t` that has already stopped before a character would
/// find the same one again and stand still. So a repeat starts one further along, and holding `;`
/// walks down the line.
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
    // Two lines of overlap, so the eye keeps its place. Vim does the same.
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
mod tests;
