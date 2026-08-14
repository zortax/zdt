//! Reading the text: lines, graphemes, and what kind of character something is.
//!
//! Everything in this crate addresses text by byte offset into a rope, and every motion and text
//! object is built out of what is here. The rules that matter and are easy to get wrong:
//!
//! * a line's *end* is before its break, and a caret in normal mode may not sit on the break;
//! * the last line of a text ending in a break is a real, empty line the caret can be on;
//! * a step is a grapheme rather than a character, so an emoji with a modifier moves once.

use std::borrow::Cow;

use ropey::Rope;
use unicode_segmentation::UnicodeSegmentation;

/// How many lines the text has.
///
/// A trailing break leaves an empty last line, which is a line the caret can sit on and which vim
/// counts. This is what everything else counts by.
#[must_use]
pub fn line_count(rope: &Rope) -> usize {
    rope.len_lines()
}

/// Which line `byte` is on.
#[must_use]
pub fn line_of(rope: &Rope, byte: usize) -> usize {
    rope.byte_to_line(byte.min(rope.len_bytes()))
}

/// The first byte of `line`.
#[must_use]
pub fn line_start(rope: &Rope, line: usize) -> usize {
    rope.line_to_byte(line.min(rope.len_lines().saturating_sub(1)))
}

/// The byte after the last character of `line`, before its break.
#[must_use]
pub fn line_end(rope: &Rope, line: usize) -> usize {
    let line = line.min(rope.len_lines().saturating_sub(1));
    let start = rope.line_to_byte(line);
    let slice = rope.line(line);
    let mut length = slice.len_bytes();
    // The break, whichever it is. The buffer holds `\n`, but a lone `\r` is still text somebody
    // could have opened.
    let text = Cow::from(slice);
    if text.ends_with('\n') {
        length -= 1;
        if text[..length].ends_with('\r') {
            length -= 1;
        }
    }
    start + length
}

/// The text of `line`, without its break.
#[must_use]
pub fn line_text(rope: &Rope, line: usize) -> Cow<'_, str> {
    Cow::from(rope.byte_slice(line_start(rope, line)..line_end(rope, line)))
}

/// Whether `line` has nothing on it.
#[must_use]
pub fn line_is_blank(rope: &Rope, line: usize) -> bool {
    line_text(rope, line).trim().is_empty()
}

/// Whether `line` is empty, which is not the same as having only spaces on it.
#[must_use]
pub fn line_is_empty(rope: &Rope, line: usize) -> bool {
    line_start(rope, line) == line_end(rope, line)
}

/// The first byte of `line` that is not a space or a tab, or the line's end.
#[must_use]
pub fn first_non_blank(rope: &Rope, line: usize) -> usize {
    let start = line_start(rope, line);
    let text = line_text(rope, line);
    let offset = text
        .find(|character: char| !character.is_whitespace())
        .unwrap_or(text.len());
    start + offset
}

/// The last byte of `line` that is not a space or a tab, or the line's start.
#[must_use]
pub fn last_non_blank(rope: &Rope, line: usize) -> usize {
    let start = line_start(rope, line);
    let text = line_text(rope, line);
    match text.rfind(|character: char| !character.is_whitespace()) {
        Some(offset) => start + offset,
        None => start,
    }
}

/// The character at `byte`, when there is one.
#[must_use]
pub fn char_at(rope: &Rope, byte: usize) -> Option<char> {
    if byte >= rope.len_bytes() {
        return None;
    }
    rope.chars_at(rope.byte_to_char(byte)).next()
}

/// The byte after the grapheme at `byte`.
///
/// Stops at the end of the text. Steps over a line break in one go, so moving right off the end
/// of a line lands at the start of the next.
#[must_use]
pub fn next_grapheme(rope: &Rope, byte: usize) -> usize {
    let length = rope.len_bytes();
    if byte >= length {
        return length;
    }
    let line = line_of(rope, byte);
    let end = line_end(rope, line);
    if byte >= end {
        // On or inside the break: the next place is the next line's start.
        return line_start(rope, (line + 1).min(rope.len_lines().saturating_sub(1)))
            .max(byte + 1)
            .min(length);
    }
    let start = line_start(rope, line);
    let text = line_text(rope, line);
    let local = byte - start;
    match text[local..].grapheme_indices(true).nth(1) {
        Some((offset, _)) => start + local + offset,
        None => end,
    }
}

/// The byte before the grapheme at `byte`.
#[must_use]
pub fn prev_grapheme(rope: &Rope, byte: usize) -> usize {
    if byte == 0 {
        return 0;
    }
    let line = line_of(rope, byte);
    let start = line_start(rope, line);
    if byte <= start {
        // At a line's start: the previous place is the end of the line above.
        return if line == 0 {
            0
        } else {
            line_end(rope, line - 1)
        };
    }
    let text = line_text(rope, line);
    let local = (byte - start).min(text.len());
    match text[..local].grapheme_indices(true).next_back() {
        Some((offset, _)) => start + offset,
        None => start,
    }
}

/// `byte`, moved to the nearest grapheme boundary at or before it.
#[must_use]
pub fn snap(rope: &Rope, byte: usize) -> usize {
    let byte = byte.min(rope.len_bytes());
    let line = line_of(rope, byte);
    let start = line_start(rope, line);
    let end = line_end(rope, line);
    if byte >= end {
        return byte.min(rope.len_bytes());
    }
    let text = line_text(rope, line);
    let local = byte - start;
    let mut last = 0;
    for (offset, _) in text.grapheme_indices(true) {
        if offset > local {
            break;
        }
        last = offset;
    }
    start + last
}

/// `byte`, kept where a normal-mode caret may sit: on a character, never on the break.
///
/// An empty line is the one place the caret sits at the line's end, because there is nowhere else
/// on it to be.
#[must_use]
pub fn clamp_normal(rope: &Rope, byte: usize) -> usize {
    let byte = snap(rope, byte.min(rope.len_bytes()));
    let line = line_of(rope, byte);
    let start = line_start(rope, line);
    let end = line_end(rope, line);
    if byte < end {
        return byte;
    }
    if end == start {
        return start;
    }
    prev_grapheme(rope, end)
}

/// What kind of character something is, for word motions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    /// A space, a tab or a break.
    Blank,
    /// A letter, a digit or an underscore.
    Word,
    /// Anything else.
    Punctuation,
}

impl Class {
    /// What `character` is.
    #[must_use]
    pub fn of(character: char) -> Self {
        if character.is_whitespace() {
            Self::Blank
        } else if character.is_alphanumeric() || character == '_' {
            Self::Word
        } else {
            Self::Punctuation
        }
    }

    /// What `character` is when the motion is a big one: everything but blanks is one class, which
    /// is what makes `W` step over `foo.bar(baz)` in one go.
    #[must_use]
    pub fn of_big(character: char) -> Self {
        if character.is_whitespace() {
            Self::Blank
        } else {
            Self::Word
        }
    }
}

/// What the character at `byte` is, treating the end of the text as blank.
#[must_use]
pub fn class_at(rope: &Rope, byte: usize, big: bool) -> Class {
    match char_at(rope, byte) {
        Some(character) if big => Class::of_big(character),
        Some(character) => Class::of(character),
        None => Class::Blank,
    }
}

/// The byte range of `line`, including its break when it has one.
#[must_use]
pub fn line_range_with_break(rope: &Rope, line: usize) -> std::ops::Range<usize> {
    let start = line_start(rope, line);
    let end = if line + 1 < rope.len_lines() {
        line_start(rope, line + 1)
    } else {
        rope.len_bytes()
    };
    start..end
}

/// The byte range of the lines `from` to `to`, taking their breaks with them.
///
/// What a linewise operator acts on. When the last line of the text is in the range and has no
/// break of its own, the break *before* the range is taken instead — otherwise deleting the last
/// line would leave an empty one behind.
#[must_use]
pub fn linewise_range(rope: &Rope, from: usize, to: usize) -> std::ops::Range<usize> {
    let last = rope.len_lines().saturating_sub(1);
    let (from, to) = (from.min(last), to.min(last));
    let (from, to) = if from <= to { (from, to) } else { (to, from) };

    let start = line_start(rope, from);
    let end = if to + 1 < rope.len_lines() {
        line_start(rope, to + 1)
    } else {
        rope.len_bytes()
    };

    if to >= last && from > 0 && end == rope.len_bytes() {
        // The end of the text, with lines above it: take the break that joins them instead.
        let above = line_end(rope, from - 1);
        return above..end;
    }
    start..end
}

#[cfg(test)]
mod tests {
    use ropey::Rope;

    use super::{
        Class, char_at, clamp_normal, class_at, first_non_blank, last_non_blank, line_count,
        line_end, line_is_blank, line_of, line_range_with_break, line_start, line_text,
        linewise_range, next_grapheme, prev_grapheme, snap,
    };

    fn rope(text: &str) -> Rope {
        Rope::from_str(text)
    }

    #[test]
    fn a_trailing_break_leaves_a_line_the_caret_can_sit_on() {
        let rope = rope("one\ntwo\n");
        assert_eq!(line_count(&rope), 3);
        assert_eq!(line_start(&rope, 2), 8);
        assert_eq!(line_end(&rope, 2), 8);
    }

    #[test]
    fn a_lines_end_is_before_its_break() {
        let rope = rope("one\ntwo\n");
        assert_eq!(line_end(&rope, 0), 3);
        assert_eq!(&line_text(&rope, 0), "one");
        assert_eq!(line_start(&rope, 1), 4);
        assert_eq!(line_end(&rope, 1), 7);
    }

    #[test]
    fn a_carriage_return_is_part_of_the_break() {
        // The buffer normally holds none, but a file that was opened with one still has to read.
        let rope = rope("one\r\ntwo");
        assert_eq!(&line_text(&rope, 0), "one");
        assert_eq!(line_end(&rope, 0), 3);
    }

    #[test]
    fn the_blanks_at_the_ends_of_a_line_are_found() {
        let rope = rope("    let x = 1;   \nnext");
        assert_eq!(first_non_blank(&rope, 0), 4);
        assert_eq!(last_non_blank(&rope, 0), 13);
        assert!(!line_is_blank(&rope, 0));
    }

    #[test]
    fn a_line_of_only_spaces_is_blank_and_has_no_non_blank() {
        let rope = rope("   \nnext");
        assert!(line_is_blank(&rope, 0));
        assert_eq!(first_non_blank(&rope, 0), 3, "the line's end");
        assert_eq!(last_non_blank(&rope, 0), 0, "the line's start");
    }

    #[test]
    fn a_step_is_a_grapheme_rather_than_a_character() {
        // A flag is two code points and one thing to step over.
        let rope = rope("a\u{1F1E9}\u{1F1EA}b");
        assert_eq!(next_grapheme(&rope, 0), 1);
        assert_eq!(next_grapheme(&rope, 1), 9);
        assert_eq!(prev_grapheme(&rope, 9), 1);
    }

    #[test]
    fn stepping_crosses_a_line_break_in_one_go() {
        let rope = rope("ab\ncd");
        assert_eq!(next_grapheme(&rope, 2), 3, "off the end of the first line");
        assert_eq!(prev_grapheme(&rope, 3), 2, "back to the first line's end");
    }

    #[test]
    fn stepping_stops_at_the_ends_of_the_text() {
        let rope = rope("ab");
        assert_eq!(prev_grapheme(&rope, 0), 0);
        assert_eq!(next_grapheme(&rope, 2), 2);
    }

    #[test]
    fn a_normal_caret_never_sits_on_the_break() {
        let rope = rope("one\ntwo\n");
        assert_eq!(clamp_normal(&rope, 3), 2, "the end of `one` is on the `e`");
        assert_eq!(clamp_normal(&rope, 0), 0);
    }

    #[test]
    fn an_empty_line_is_where_the_caret_sits_at_the_end() {
        // There is nowhere else on it to be.
        let rope = rope("one\n\ntwo\n");
        assert_eq!(clamp_normal(&rope, 4), 4);
    }

    #[test]
    fn snapping_lands_on_a_boundary() {
        let rope = rope("a\u{1F1E9}\u{1F1EA}b");
        assert_eq!(snap(&rope, 3), 1, "inside the flag");
        assert_eq!(snap(&rope, 1), 1);
        assert_eq!(snap(&rope, 9), 9);
    }

    #[test]
    fn a_character_is_read_at_a_byte() {
        let rope = rope("héllo");
        assert_eq!(char_at(&rope, 0), Some('h'));
        assert_eq!(char_at(&rope, 1), Some('é'));
        assert_eq!(char_at(&rope, 99), None);
    }

    #[test]
    fn classes_are_what_word_motions_step_between() {
        let rope = rope("foo.bar baz");
        assert_eq!(class_at(&rope, 0, false), Class::Word);
        assert_eq!(class_at(&rope, 3, false), Class::Punctuation);
        assert_eq!(class_at(&rope, 7, false), Class::Blank);
        // A big word is everything that is not a blank.
        assert_eq!(class_at(&rope, 3, true), Class::Word);
    }

    #[test]
    fn a_line_range_takes_its_break_with_it() {
        let rope = rope("one\ntwo\nthree");
        assert_eq!(line_range_with_break(&rope, 0), 0..4);
        assert_eq!(line_range_with_break(&rope, 2), 8..13, "no break to take");
    }

    #[test]
    fn a_linewise_range_over_the_last_line_takes_the_break_above_it() {
        // Otherwise `dd` on the last line would leave an empty one where it was.
        let rope = rope("one\ntwo\nthree");
        assert_eq!(linewise_range(&rope, 2, 2), 7..13);
        assert_eq!(linewise_range(&rope, 0, 0), 0..4);
        assert_eq!(linewise_range(&rope, 0, 1), 0..8);
    }

    #[test]
    fn a_linewise_range_over_the_only_line_is_the_whole_text() {
        let rope = rope("only");
        assert_eq!(linewise_range(&rope, 0, 0), 0..4);
    }

    #[test]
    fn a_linewise_range_is_the_same_either_way_round() {
        let rope = rope("one\ntwo\nthree\n");
        assert_eq!(linewise_range(&rope, 0, 1), linewise_range(&rope, 1, 0));
    }

    #[test]
    fn a_byte_past_the_end_is_still_on_the_last_line() {
        let rope = rope("one\ntwo");
        assert_eq!(line_of(&rope, 999), 1);
        assert_eq!(clamp_normal(&rope, 999), 6);
    }
}
