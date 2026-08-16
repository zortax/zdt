//! Between the editor's offsets and the protocol's positions.
//!
//! The editor counts bytes. The protocol counts lines and characters, and by default a character
//! is a UTF-16 code unit. That is neither a byte nor a `char`. A server that says the error is at
//! character 12 of line 3 means twelve UTF-16 units, so on a line with an emoji in it, twelve of
//! anything else lands in the wrong place.
//!
//! Every conversion in this file works against the rope, so every one is exact. That matters more
//! than it sounds. A diagnostic underline one character wide in the wrong place is worse than
//! none, and a completion that replaces the wrong range corrupts the text.

use lsp_types::{Position, Range};
use ropey::Rope;

/// Which units a server counts characters in.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Encoding {
    /// UTF-16 code units, which is what the protocol says when a server says nothing.
    #[default]
    Utf16,
    /// Bytes, which some servers ask for and which is what the editor already uses.
    Utf8,
    /// Unicode scalar values.
    Utf32,
}

impl Encoding {
    /// The encoding a server named, when it named one this understands.
    #[must_use]
    pub fn of(named: Option<&lsp_types::PositionEncodingKind>) -> Self {
        match named.map(lsp_types::PositionEncodingKind::as_str) {
            Some("utf-8") => Self::Utf8,
            Some("utf-32") => Self::Utf32,
            _ => Self::Utf16,
        }
    }
}

/// Where `byte` is, as the protocol would say it.
///
/// A byte past the end answers the last position. An offset that has moved under a request is a
/// race, and propagating it as an error helps nobody.
#[must_use]
pub fn position_of(rope: &Rope, byte: usize, encoding: Encoding) -> Position {
    let byte = byte.min(rope.len_bytes());
    let line = rope.byte_to_line(byte);
    let line_start = rope.line_to_byte(line);
    let column = measure(&rope.byte_slice(line_start..byte).to_string(), encoding);
    Position {
        line: line as u32,
        character: column as u32,
    }
}

/// Which byte `position` is.
///
/// A line or a character past the end is clamped, for the same reason.
#[must_use]
pub fn byte_of(rope: &Rope, position: Position, encoding: Encoding) -> usize {
    let line = (position.line as usize).min(rope.len_lines().saturating_sub(1));
    let line_start = rope.line_to_byte(line);
    let text = rope.line(line).to_string();
    line_start + advance(&text, position.character as usize, encoding)
}

/// The byte range `range` covers.
#[must_use]
pub fn range_of(rope: &Rope, range: Range, encoding: Encoding) -> std::ops::Range<usize> {
    let start = byte_of(rope, range.start, encoding);
    let end = byte_of(rope, range.end, encoding);
    // A server that hands back an inverted range means the two ends, whichever way round.
    if start <= end { start..end } else { end..start }
}

/// The protocol range a byte range comes to.
#[must_use]
pub fn lsp_range(rope: &Rope, range: std::ops::Range<usize>, encoding: Encoding) -> Range {
    Range {
        start: position_of(rope, range.start, encoding),
        end: position_of(rope, range.end, encoding),
    }
}

/// How long `text` is in `encoding`'s units.
fn measure(text: &str, encoding: Encoding) -> usize {
    match encoding {
        Encoding::Utf8 => text.len(),
        Encoding::Utf16 => text.chars().map(char::len_utf16).sum(),
        Encoding::Utf32 => text.chars().count(),
    }
}

/// How many bytes into `text` `units` of `encoding` reach.
fn advance(text: &str, units: usize, encoding: Encoding) -> usize {
    if units == 0 {
        return 0;
    }
    match encoding {
        // Clamped to a character boundary: a server counting bytes can still name one in the
        // middle of a character when it and the editor disagree about the text.
        Encoding::Utf8 => {
            let mut at = units.min(text.len());
            while at > 0 && !text.is_char_boundary(at) {
                at -= 1;
            }
            at
        }
        // A position inside a surrogate pair rounds *back* to where the character begins. Both
        // directions are wrong, because the server and the editor disagree about the text.
        // Rounding back can only ever include less. Rounding forward can step over a character
        // that was meant to be edited.
        Encoding::Utf16 => {
            let mut counted = 0;
            for (offset, character) in text.char_indices() {
                if counted >= units {
                    return offset;
                }
                counted += character.len_utf16();
                if counted > units {
                    return offset;
                }
            }
            text.len()
        }
        Encoding::Utf32 => text
            .char_indices()
            .nth(units)
            .map_or(text.len(), |(offset, _)| offset),
    }
}

/// The file a URI names, when it names one on this machine.
#[must_use]
pub fn path_of(uri: &lsp_types::Url) -> Option<std::path::PathBuf> {
    uri.to_file_path().ok()
}

/// The URI a path comes to.
#[must_use]
pub fn uri_of(path: &std::path::Path) -> Option<lsp_types::Url> {
    lsp_types::Url::from_file_path(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rope(text: &str) -> Rope {
        Rope::from_str(text)
    }

    #[test]
    fn ascii_is_the_same_in_every_encoding() {
        let text = rope("hello\nworld\n");
        for encoding in [Encoding::Utf8, Encoding::Utf16, Encoding::Utf32] {
            let at = position_of(&text, 8, encoding);
            assert_eq!(at, Position::new(1, 2), "{encoding:?}");
            assert_eq!(byte_of(&text, at, encoding), 8, "{encoding:?}");
        }
    }

    #[test]
    fn an_emoji_is_two_units_in_the_protocols_own_encoding() {
        // "🦀" is four bytes, two UTF-16 units and one scalar.
        let text = rope("🦀x");

        assert_eq!(position_of(&text, 4, Encoding::Utf16).character, 2);
        assert_eq!(position_of(&text, 4, Encoding::Utf8).character, 4);
        assert_eq!(position_of(&text, 4, Encoding::Utf32).character, 1);
    }

    #[test]
    fn a_position_after_an_emoji_comes_back_to_the_right_byte() {
        let text = rope("🦀x");

        assert_eq!(byte_of(&text, Position::new(0, 2), Encoding::Utf16), 4);
        assert_eq!(byte_of(&text, Position::new(0, 1), Encoding::Utf32), 4);
        assert_eq!(byte_of(&text, Position::new(0, 4), Encoding::Utf8), 4);
    }

    #[test]
    fn a_position_in_the_middle_of_a_character_lands_on_its_start() {
        let text = rope("🦀x");
        // One UTF-16 unit into a surrogate pair is not a place; the byte before it is.
        assert_eq!(byte_of(&text, Position::new(0, 1), Encoding::Utf16), 0);
        // The same for a byte-counting server that has miscounted.
        assert_eq!(byte_of(&text, Position::new(0, 2), Encoding::Utf8), 0);
    }

    #[test]
    fn positions_past_the_end_are_clamped_rather_than_refused() {
        let text = rope("one\ntwo\n");
        assert_eq!(
            position_of(&text, 9_000, Encoding::Utf16),
            Position::new(2, 0)
        );
        assert_eq!(
            byte_of(&text, Position::new(9_000, 9_000), Encoding::Utf16),
            text.len_bytes()
        );
    }

    #[test]
    fn a_range_survives_the_round_trip() {
        let text = rope("fn main() {\n    println!(\"🦀\");\n}\n");
        let bytes = 16..24;
        let range = lsp_range(&text, bytes.clone(), Encoding::Utf16);
        assert_eq!(range_of(&text, range, Encoding::Utf16), bytes);
    }

    #[test]
    fn an_inverted_range_means_the_two_ends() {
        let text = rope("hello world");
        let inverted = Range {
            start: Position::new(0, 8),
            end: Position::new(0, 2),
        };
        assert_eq!(range_of(&text, inverted, Encoding::Utf16), 2..8);
    }

    #[test]
    fn the_encoding_a_server_names_is_the_one_used() {
        use lsp_types::PositionEncodingKind;

        assert_eq!(
            Encoding::of(None),
            Encoding::Utf16,
            "the protocol's default"
        );
        assert_eq!(
            Encoding::of(Some(&PositionEncodingKind::UTF8)),
            Encoding::Utf8
        );
        assert_eq!(
            Encoding::of(Some(&PositionEncodingKind::UTF32)),
            Encoding::Utf32
        );
        assert_eq!(
            Encoding::of(Some(&PositionEncodingKind::new("utf-64"))),
            Encoding::Utf16,
            "and something nobody has heard of falls back to the default"
        );
    }

    #[test]
    fn a_path_survives_becoming_a_uri() {
        let path = std::path::Path::new("/home/someone/a file.rs");
        let uri = uri_of(path).expect("an absolute path is a uri");
        assert_eq!(path_of(&uri).as_deref(), Some(path), "spaces and all");
    }
}
