//! Between a place in the text and a place in the grid.
//!
//! One rope line per grid line, so the two agree on which line is which and only the column has
//! to be worked out. A grid column counts cells, and a wide character covers two of them, so the
//! column of a byte is the width of the line before it.

use unicode_segmentation::UnicodeSegmentation as _;
use unicode_width::UnicodeWidthStr;
use zgui_terminal::GridPoint;

/// Which line and column `byte` is at.
///
/// A byte past the end of the text answers the last place there is, which is what clamping a
/// caret to a text that has shrunk under it comes to.
#[must_use]
pub fn point_of(rope: &ropey::Rope, byte: usize) -> GridPoint {
    let byte = byte.min(rope.len_bytes());
    let line = rope.byte_to_line(byte);
    let start = rope.line_to_byte(line);
    let before = rope.byte_slice(start..byte).to_string();
    GridPoint {
        line,
        column: width_of(&before),
    }
}

/// Which byte the character covering the cell at `point` starts at.
///
/// The cell a wide character covers is that character, so pressing either half of one puts the
/// caret on it. A column past the end of its line answers the end of that line: the cells past
/// the last character are blanks the terminal draws and the text does not hold.
#[must_use]
pub fn byte_of(rope: &ropey::Rope, point: GridPoint) -> usize {
    let line = point.line.min(rope.len_lines().saturating_sub(1));
    let start = rope.line_to_byte(line);
    let text = rope.line(line).to_string();
    let text = text.strip_suffix('\n').unwrap_or(&text);

    let mut column = 0u16;
    for (offset, character) in text.char_indices() {
        let width = width_of(character.encode_utf8(&mut [0; 4]));
        // A combining mark is drawn on the cell before it and is never a cell of its own.
        if width == 0 {
            continue;
        }
        if point.column < column.saturating_add(width) {
            return start + offset;
        }
        column = column.saturating_add(width);
    }
    start + text.len()
}

/// Which cell the grapheme at `column` on `line` starts at.
///
/// The engine counts a column in graphemes and the grid counts one in cells, so a wide character
/// before it moves it two. A column past the end of the line counts one cell for each blank the
/// terminal draws beyond it, which is what lets a block keep its shape over lines too short to
/// reach it.
#[must_use]
pub fn cell_of(rope: &ropey::Rope, line: usize, column: usize) -> u16 {
    let line = line.min(rope.len_lines().saturating_sub(1));
    let text = rope.line(line).to_string();
    let text = text.strip_suffix('\n').unwrap_or(&text);

    let mut cells = 0u16;
    let mut graphemes = 0usize;
    for grapheme in text.graphemes(true) {
        if graphemes == column {
            return cells;
        }
        cells = cells.saturating_add(width_of(grapheme));
        graphemes += 1;
    }
    cells.saturating_add(column.saturating_sub(graphemes).min(usize::from(u16::MAX)) as u16)
}

/// How many cells `text` covers.
fn width_of(text: &str) -> u16 {
    UnicodeWidthStr::width(text).min(usize::from(u16::MAX)) as u16
}

#[cfg(test)]
mod tests {
    use super::{byte_of, point_of};
    use zgui_terminal::GridPoint;

    fn rope(text: &str) -> ropey::Rope {
        ropey::Rope::from_str(text)
    }

    #[test]
    fn a_byte_and_a_cell_name_the_same_place() {
        let rope = rope("hello\nworld\n");
        let at = point_of(&rope, 8);
        assert_eq!(at, GridPoint::new(1, 2));
        assert_eq!(byte_of(&rope, at), 8);
    }

    #[test]
    fn a_wide_character_covers_two_cells() {
        // The grid draws one glyph across two cells, so everything after it is two columns
        // further along than its bytes suggest.
        let rope = rope("a漢b\n");
        assert_eq!(point_of(&rope, 0), GridPoint::new(0, 0));
        assert_eq!(point_of(&rope, 1), GridPoint::new(0, 1));
        assert_eq!(point_of(&rope, 4), GridPoint::new(0, 3));
        assert_eq!(byte_of(&rope, GridPoint::new(0, 3)), 4);
        // The cell the wide character covers is the wide character.
        assert_eq!(byte_of(&rope, GridPoint::new(0, 2)), 1);
    }

    #[test]
    fn a_grapheme_column_becomes_the_cell_it_starts_at() {
        use super::cell_of;

        let rope = rope("a\u{6f22}bc\n");
        assert_eq!(cell_of(&rope, 0, 0), 0);
        assert_eq!(cell_of(&rope, 0, 1), 1);
        // Past the wide character, which covers two cells.
        assert_eq!(cell_of(&rope, 0, 2), 3);
        assert_eq!(cell_of(&rope, 0, 3), 4);
    }

    #[test]
    fn a_column_past_the_end_of_a_line_counts_the_blanks() {
        use super::cell_of;

        // What lets a block selection stay a rectangle over a line too short to reach it.
        let rope = rope("ab\nlonger line\n");
        assert_eq!(cell_of(&rope, 0, 2), 2);
        assert_eq!(cell_of(&rope, 0, 6), 6);
    }

    #[test]
    fn a_column_past_the_end_is_the_end_of_the_line() {
        let rope = rope("ab\nlonger\n");
        assert_eq!(byte_of(&rope, GridPoint::new(0, 40)), 2);
    }

    #[test]
    fn a_line_past_the_end_is_the_last_line() {
        let rope = rope("only\n");
        // A rope ending in a newline has an empty last line, which is where a caret past the end
        // belongs.
        assert_eq!(byte_of(&rope, GridPoint::new(99, 0)), 5);
    }

    #[test]
    fn an_empty_line_is_its_own_start() {
        let rope = rope("a\n\nb\n");
        assert_eq!(byte_of(&rope, GridPoint::new(1, 0)), 2);
        assert_eq!(point_of(&rope, 2), GridPoint::new(1, 0));
    }
}
