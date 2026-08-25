//! One line of code, drawn as a run of coloured spans.
//!
//! The container stays a block; the pieces are inline `text` elements, so the line shapes as
//! one line however many colours it carries. Overlapping captures resolve exactly as the
//! editor's painter resolves them: the last span starting at or before a byte wins, and a byte
//! past that span's end is plain.

use zgui::view;
use zgui_editor::syntax::LineSpan;

use crate::Highlights;
use zdt_view::Erase;

/// The view of one line: coloured when there are spans for it, plain otherwise.
#[must_use]
pub fn line_view(text: &str, marks: Option<(&Highlights, u32)>) -> zgui::view::AnyView {
    let Some((held, number)) = marks else {
        return plain(text);
    };
    let spans = held.spans(number);
    if spans.is_empty() {
        return plain(text);
    }

    segments(text, spans)
        .into_iter()
        .map(|(range, capture)| {
            let piece = text[range].to_owned();
            match capture.and_then(|index| held.class(index)) {
                Some(class) => view! { text(class = class) {{piece}} }.any(),
                None => view! { text {{piece}} }.any(),
            }
        })
        .collect::<Vec<_>>()
        .any()
}

/// The line as one uncoloured piece.
fn plain(text: &str) -> zgui::view::AnyView {
    let piece = text.to_owned();
    view! { text {{piece}} }.any()
}

/// The line cut at every span edge, each piece with the capture that wins it.
fn segments(text: &str, spans: &[LineSpan]) -> Vec<(std::ops::Range<usize>, Option<u16>)> {
    let length = text.len();
    let mut edges: Vec<usize> = vec![0, length];
    for (start, end, _) in spans {
        for edge in [*start as usize, *end as usize] {
            let edge = edge.min(length);
            if text.is_char_boundary(edge) {
                edges.push(edge);
            }
        }
    }
    edges.sort_unstable();
    edges.dedup();

    edges
        .windows(2)
        .filter(|pair| pair[0] < pair[1])
        .map(|pair| (pair[0]..pair[1], winner(spans, pair[0] as u32)))
        .collect()
}

/// The capture that colours the byte at `at`: the last span starting at or before it, when
/// that span also reaches past it.
fn winner(spans: &[LineSpan], at: u32) -> Option<u16> {
    let index = spans.partition_point(|(start, _, _)| *start <= at);
    let (_, end, capture) = spans.get(index.checked_sub(1)?)?;
    (at < *end).then_some(*capture)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_cut_the_line_into_pieces() {
        let text = "let x = 1;";
        let spans: &[LineSpan] = &[(0, 3, 7), (8, 9, 2)];
        let cut = segments(text, spans);
        assert_eq!(
            cut,
            vec![
                (0..3, Some(7)),
                (3..8, None),
                (8..9, Some(2)),
                (9..10, None),
            ]
        );
    }

    #[test]
    fn the_last_starting_span_wins_and_its_end_is_honoured() {
        // An outer span under an inner one: past the inner's end, the byte is plain, exactly
        // as the editor paints it.
        let spans: &[LineSpan] = &[(0, 10, 1), (2, 4, 2)];
        assert_eq!(winner(spans, 0), Some(1));
        assert_eq!(winner(spans, 2), Some(2));
        assert_eq!(winner(spans, 5), None);
    }

    #[test]
    fn ends_past_the_line_are_clamped() {
        let text = "short";
        let spans: &[LineSpan] = &[(0, u32::MAX, 3)];
        let cut = segments(text, spans);
        assert_eq!(cut, vec![(0..5, Some(3))]);
    }

    #[test]
    fn edges_inside_a_character_are_left_out() {
        let text = "a\u{2014}b";
        let spans: &[LineSpan] = &[(0, 2, 1)];
        let cut = segments(text, spans);
        assert_eq!(cut, vec![(0..5, Some(1))]);
    }
}
