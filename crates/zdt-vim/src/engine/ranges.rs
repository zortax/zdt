//! Turning a motion into a range of bytes, and the text operations over one.

use super::*;

/// Where the caret lands after whole lines are deleted.
///
/// On the first non-blank of the line that now sits where they were. On the line above when they
/// were the last ones. That is what leaves `dd` on the last line with the caret on text, and away
/// from the end of the buffer.
pub(super) fn linewise_caret(rope: &Rope, range: &std::ops::Range<usize>) -> usize {
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
pub(super) fn operator_range(rope: &Rope, from: usize, target: Target) -> std::ops::Range<usize> {
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
pub(super) fn indent_lines(
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
                // An empty line stays empty. Indent never fills one with spaces.
                None
            } else {
                Some((start..start, INDENT.to_owned()))
            }
        })
        .collect()
}

/// Every letter's case turned over.
pub(super) fn swap_case(text: &str) -> String {
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

/// The bytes a block selection is, one range per line.
///
/// A column past the end of a line has no bytes, so a line that stops inside the rectangle gives
/// up only what is there. That is what keeps a yanked block free of whitespace nobody typed.
pub(super) fn block_selections(
    rope: &Rope,
    lines: std::ops::Range<usize>,
    columns: std::ops::Range<usize>,
) -> Vec<Selection> {
    lines
        .map(|line| {
            let start = motion::byte_at_column(rope, line, columns.start);
            let end = motion::byte_at_column(rope, line, columns.end);
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
