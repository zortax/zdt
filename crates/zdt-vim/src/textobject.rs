//! What `iw`, `a"`, `i(` and the rest select.
//!
//! A text object answers a byte range rather than a place to go, which is the whole difference
//! between it and a motion: `diw` deletes the word the caret is *in*, wherever in it the caret
//! happens to be, and needs no knowledge of which way to move first.
//!
//! Ranges are half-open and end-exclusive throughout, so the caller never has to ask which of the
//! three vim measurements applies.

use std::ops::Range;

use ropey::Rope;

use crate::text::{self, Class};

/// The word the caret is in.
///
/// `around` takes the blanks after the word too — the whole difference between `diw` on
/// `hello world` leaving two spaces and `daw` leaving one. With no blanks after it, the ones
/// before it are taken instead, which is what makes `daw` on the last word of a line tidy.
#[must_use]
pub fn word(rope: &Rope, at: usize, big: bool, around: bool) -> Option<Range<usize>> {
    let length = rope.len_bytes();
    if length == 0 {
        return None;
    }
    let at = at.min(text::prev_grapheme(rope, length));
    let class = text::class_at(rope, at, big);

    let mut start = at;
    while start > 0 {
        let previous = text::prev_grapheme(rope, start);
        if text::class_at(rope, previous, big) != class || crosses_line(rope, previous, start) {
            break;
        }
        start = previous;
    }

    let mut end = at;
    loop {
        let next = text::next_grapheme(rope, end);
        if next >= length
            || text::class_at(rope, next, big) != class
            || crosses_line(rope, end, next)
        {
            end = next.min(length);
            break;
        }
        end = next;
    }

    if !around {
        return Some(start..end);
    }

    // The blanks after it, or the ones before it when there are none.
    let mut after = end;
    while after < length
        && text::class_at(rope, after, big) == Class::Blank
        && !crosses_line(rope, text::prev_grapheme(rope, after), after)
    {
        after = text::next_grapheme(rope, after);
    }
    if after > end {
        return Some(start..after);
    }

    let mut before = start;
    while before > 0 {
        let previous = text::prev_grapheme(rope, before);
        if text::class_at(rope, previous, big) != Class::Blank
            || crosses_line(rope, previous, before)
        {
            break;
        }
        before = previous;
    }
    Some(before..end)
}

/// Whether a line break sits between two adjacent positions.
fn crosses_line(rope: &Rope, one: usize, two: usize) -> bool {
    text::line_of(rope, one) != text::line_of(rope, two)
}

/// The paragraph the caret is in: the run of lines around it that are all blank or all not.
///
/// `around` takes the blank lines after it, which is what `dap` does.
#[must_use]
pub fn paragraph(rope: &Rope, at: usize, around: bool) -> Option<Range<usize>> {
    let last = rope.len_lines().saturating_sub(1);
    let line = text::line_of(rope, at);
    let blank = text::line_is_empty(rope, line);

    let mut first = line;
    while first > 0 && text::line_is_empty(rope, first - 1) == blank {
        first -= 1;
    }
    let mut end = line;
    while end < last && text::line_is_empty(rope, end + 1) == blank {
        end += 1;
    }

    if around {
        // The run of the other kind after it — which for a paragraph of text is the blank lines
        // that separate it from the next one.
        while end < last && text::line_is_empty(rope, end + 1) != blank {
            end += 1;
        }
    }

    Some(text::line_start(rope, first)..line_end_with_break(rope, end))
}

/// The end of `line`, taking its break when it has one.
fn line_end_with_break(rope: &Rope, line: usize) -> usize {
    if line + 1 < rope.len_lines() {
        text::line_start(rope, line + 1)
    } else {
        rope.len_bytes()
    }
}

/// The sentence the caret is in.
///
/// A sentence ends at `.`, `!` or `?` followed by a space or a break. Rough, and the same rough as
/// every editor's: prose is not a grammar this can win at.
#[must_use]
pub fn sentence(rope: &Rope, at: usize, around: bool) -> Option<Range<usize>> {
    let length = rope.len_bytes();
    if length == 0 {
        return None;
    }

    let ends_here = |byte: usize| {
        matches!(text::char_at(rope, byte), Some('.' | '!' | '?'))
            && text::char_at(rope, text::next_grapheme(rope, byte))
                .is_none_or(|next| next.is_whitespace())
    };

    let mut start = at.min(length);
    while start > 0 {
        let previous = text::prev_grapheme(rope, start);
        if ends_here(previous) {
            break;
        }
        start = previous;
    }
    // Past the blanks the previous sentence's stop left behind.
    while start < length && text::char_at(rope, start).is_some_and(char::is_whitespace) {
        start = text::next_grapheme(rope, start);
    }

    let mut end = at.min(length);
    while end < length && !ends_here(end) {
        end = text::next_grapheme(rope, end);
    }
    end = text::next_grapheme(rope, end).min(length);

    if around {
        while end < length && text::char_at(rope, end).is_some_and(|c| c == ' ' || c == '\t') {
            end = text::next_grapheme(rope, end);
        }
    }

    Some(start..end)
}

/// What is inside a pair of `quote` characters, on the caret's line.
///
/// The line rather than the file, because a quote is nearly always closed on the line it opens on
/// and searching the whole file for one turns a typo into a very large selection.
#[must_use]
pub fn quote(rope: &Rope, at: usize, quote: char, around: bool) -> Option<Range<usize>> {
    let line = text::line_of(rope, at);
    let start = text::line_start(rope, line);
    let text = text::line_text(rope, line);

    // Where every unescaped quote on the line is.
    let mut marks: Vec<usize> = Vec::new();
    let mut escaped = false;
    for (offset, character) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if character == quote {
            marks.push(start + offset);
        }
    }
    if marks.len() < 2 {
        return None;
    }

    // The pair the caret is in, or the first pair after it — which is what makes `ci"` work with
    // the caret before the string as well as inside it.
    for pair in marks.chunks_exact(2) {
        let (open, close) = (pair[0], pair[1]);
        if at <= close {
            let inner = text::next_grapheme(rope, open)..close;
            return Some(if around {
                open..text::next_grapheme(rope, close)
            } else {
                inner
            });
        }
    }
    None
}

/// The pair matching `open` — `(`, `[`, `{` or `<` — that the caret is inside.
///
/// Searches the whole text, because a brace really does span lines and a function body is the
/// commonest thing anybody selects.
#[must_use]
pub fn pair(rope: &Rope, at: usize, open: char, around: bool) -> Option<Range<usize>> {
    let close = match open {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '<' => '>',
        _ => return None,
    };
    let length = rope.len_bytes();

    // The opening bracket this position is inside, counting nesting on the way out.
    let mut depth = 0i32;
    let mut start = at.min(length);
    let opening = loop {
        let character = text::char_at(rope, start);
        if character == Some(close) && start != at {
            depth += 1;
        } else if character == Some(open) {
            if depth == 0 {
                break start;
            }
            depth -= 1;
        }
        if start == 0 {
            return None;
        }
        start = text::prev_grapheme(rope, start);
    };

    // Its partner.
    let mut depth = 0i32;
    let mut end = opening;
    let closing = loop {
        let character = text::char_at(rope, end);
        if character == Some(open) {
            depth += 1;
        } else if character == Some(close) {
            depth -= 1;
            if depth == 0 {
                break end;
            }
        }
        let next = text::next_grapheme(rope, end);
        if next >= length {
            return None;
        }
        end = next;
    };

    Some(if around {
        opening..text::next_grapheme(rope, closing)
    } else {
        text::next_grapheme(rope, opening)..closing
    })
}

#[cfg(test)]
mod tests {
    use ropey::Rope;

    use super::{pair, paragraph, quote, sentence, word};

    fn rope(text: &str) -> Rope {
        Rope::from_str(text)
    }

    fn taken(text: &str, range: std::ops::Range<usize>) -> String {
        text[range].to_owned()
    }

    #[test]
    fn iw_is_the_word_the_caret_is_in() {
        let text = "hello brave world";
        let rope = rope(text);
        for at in 6..11 {
            assert_eq!(
                taken(text, word(&rope, at, false, false).unwrap()),
                "brave",
                "from {at}"
            );
        }
    }

    #[test]
    fn aw_takes_the_space_after_the_word() {
        // The whole difference between `diw` leaving two spaces and `daw` leaving one.
        let text = "hello brave world";
        let rope = rope(text);
        assert_eq!(taken(text, word(&rope, 6, false, true).unwrap()), "brave ");
    }

    #[test]
    fn aw_on_the_last_word_takes_the_space_before_it() {
        // Otherwise `daw` at the end of a line would leave a trailing space behind.
        let text = "hello world";
        let rope = rope(text);
        assert_eq!(taken(text, word(&rope, 6, false, true).unwrap()), " world");
    }

    #[test]
    fn iw_on_a_blank_is_the_run_of_blanks() {
        let text = "a   b";
        let rope = rope(text);
        assert_eq!(taken(text, word(&rope, 2, false, false).unwrap()), "   ");
    }

    #[test]
    fn a_small_word_stops_at_punctuation_and_a_big_one_does_not() {
        let text = "foo.bar baz";
        let rope = rope(text);
        assert_eq!(taken(text, word(&rope, 0, false, false).unwrap()), "foo");
        assert_eq!(taken(text, word(&rope, 0, true, false).unwrap()), "foo.bar");
    }

    #[test]
    fn a_word_does_not_run_over_a_line_break() {
        let text = "foo\nbar";
        let rope = rope(text);
        assert_eq!(taken(text, word(&rope, 0, false, false).unwrap()), "foo");
        assert_eq!(taken(text, word(&rope, 4, false, false).unwrap()), "bar");
    }

    #[test]
    fn ip_is_the_run_of_lines_around_the_caret() {
        let text = "one\ntwo\n\nthree\nfour\n";
        let rope = rope(text);
        assert_eq!(
            taken(text, paragraph(&rope, 0, false).unwrap()),
            "one\ntwo\n"
        );
        assert_eq!(
            taken(text, paragraph(&rope, 9, false).unwrap()),
            "three\nfour\n"
        );
    }

    #[test]
    fn ap_takes_the_blank_lines_after_it() {
        let text = "one\ntwo\n\n\nthree\n";
        let rope = rope(text);
        assert_eq!(
            taken(text, paragraph(&rope, 0, true).unwrap()),
            "one\ntwo\n\n\n"
        );
    }

    #[test]
    fn ip_on_a_blank_run_is_the_blank_run() {
        let text = "one\n\n\ntwo\n";
        let rope = rope(text);
        assert_eq!(taken(text, paragraph(&rope, 4, false).unwrap()), "\n\n");
    }

    #[test]
    fn quotes_take_what_is_between_them() {
        let text = "let name = \"hello there\";";
        let rope = rope(text);
        assert_eq!(
            taken(text, quote(&rope, 14, '"', false).unwrap()),
            "hello there"
        );
        assert_eq!(
            taken(text, quote(&rope, 14, '"', true).unwrap()),
            "\"hello there\""
        );
    }

    #[test]
    fn a_caret_before_the_string_still_finds_it() {
        // `ci"` with the caret at the start of the line is what people actually type.
        let text = "let name = \"hello\";";
        let rope = rope(text);
        assert_eq!(taken(text, quote(&rope, 0, '"', false).unwrap()), "hello");
    }

    #[test]
    fn an_escaped_quote_is_not_an_end() {
        let text = "\"a \\\" b\"";
        let rope = rope(text);
        assert_eq!(
            taken(text, quote(&rope, 1, '"', false).unwrap()),
            "a \\\" b"
        );
    }

    #[test]
    fn a_quote_is_looked_for_on_one_line_only() {
        // Searching the whole file would turn one unclosed quote into an enormous selection.
        let text = "\"open\nclose\"";
        let rope = rope(text);
        assert_eq!(quote(&rope, 7, '"', false), None);
    }

    #[test]
    fn brackets_take_what_is_between_them() {
        let text = "fn main() { let x = 1; }";
        let rope = rope(text);
        assert_eq!(
            taken(text, pair(&rope, 15, '{', false).unwrap()),
            " let x = 1; "
        );
        assert_eq!(
            taken(text, pair(&rope, 15, '{', true).unwrap()),
            "{ let x = 1; }"
        );
    }

    #[test]
    fn brackets_span_lines() {
        let text = "fn main() {\n    let x = 1;\n}\n";
        let rope = rope(text);
        assert_eq!(
            taken(text, pair(&rope, 16, '{', false).unwrap()),
            "\n    let x = 1;\n"
        );
    }

    #[test]
    fn a_nested_bracket_takes_the_one_it_is_in() {
        let text = "a(b(c)d)e";
        let rope = rope(text);
        assert_eq!(taken(text, pair(&rope, 4, '(', false).unwrap()), "c");
        assert_eq!(taken(text, pair(&rope, 2, '(', false).unwrap()), "b(c)d");
    }

    #[test]
    fn a_caret_outside_any_bracket_selects_nothing() {
        let text = "no brackets here";
        let rope = rope(text);
        assert_eq!(pair(&rope, 5, '(', false), None);
    }

    #[test]
    fn an_unclosed_bracket_selects_nothing() {
        let text = "fn main() {";
        let rope = rope(text);
        assert_eq!(pair(&rope, 10, '{', false), None);
    }

    #[test]
    fn a_sentence_ends_at_a_stop_followed_by_a_space() {
        let text = "One thing. Then another. And a third.";
        let rope = rope(text);
        assert_eq!(
            taken(text, sentence(&rope, 2, false).unwrap()),
            "One thing."
        );
        assert_eq!(
            taken(text, sentence(&rope, 12, false).unwrap()),
            "Then another."
        );
        assert_eq!(
            taken(text, sentence(&rope, 2, true).unwrap()),
            "One thing. "
        );
    }

    #[test]
    fn nothing_is_selected_in_an_empty_text() {
        let rope = rope("");
        assert_eq!(word(&rope, 0, false, false), None);
        assert_eq!(sentence(&rope, 0, false), None);
    }
}
