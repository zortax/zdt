//! The picker, drawn.
//!
//! A prompt across the top, the matches down the left, and what the caret is on shown on the
//! right.
//!
//! # The preview
//!
//! One editor, mounted once and kept for as long as the picker is open, whose text is replaced as
//! the caret moves. There is one for the whole picker, and it is never rebuilt per selection. An
//! editor costs a syntax worker and a first parse, and paying that twenty times while somebody
//! holds `<C-j>` is the difference between a picker that keeps up and one that does not.
//!
//! The read is debounced for the same reason. Walking a list asks to read the file it stops on,
//! and none of the ones it passes over.

mod matches;
mod modal;
mod preview;

pub use crate::picker::view::modal::{Picker, PickerProps};

pub(crate) use crate::picker::view::matches::MatchesProps;
pub(crate) use crate::picker::view::preview::PreviewProps;

use std::time::Duration;

/// How tall one row is. The list is told, and measures nothing.
const ROW: f32 = 22.0;

/// How long the caret has to rest on a row before its file is read.
const PREVIEW_DEBOUNCE: Duration = Duration::from_millis(40);

/// The layer the previewed match is banded in.
const MATCH_LAYER: &str = "picker-match";

/// Puts the previewed file at the place the row stands for, and picks the match out.
///
/// Centred, and not merely visible. A hit at the bottom of the preview with nothing under it
/// reads as the end of the file.
fn show_place(handle: &zgui_editor::EditorHandle, preview: &crate::picker::Preview) {
    let Some(line) = preview.line else {
        handle.clear_decorations(MATCH_LAYER);
        handle.command(zgui_editor::Command::Scroll(
            zgui_editor::ScrollCmd::ToLine(0),
        ));
        return;
    };

    let (at, matched) = handle.query(|snapshot| {
        let rope = snapshot.rope();
        let line = (line as usize)
            .saturating_sub(1)
            .min(rope.len_lines().saturating_sub(1));
        let start = rope.char_to_byte(rope.line_to_char(line));
        // The match is a range within the line, which is where the searcher measured it.
        let matched = preview.matched.as_ref().map(|range| {
            let end = rope.len_bytes();
            (start + range.start).min(end)..(start + range.end).min(end)
        });
        (start, matched)
    });

    handle.command(zgui_editor::Command::SetSelections {
        selections: vec![zgui_editor::Selection::caret(at)],
        primary: 0,
    });
    handle.command(zgui_editor::Command::Scroll(
        zgui_editor::ScrollCmd::CursorCenter,
    ));

    match matched.filter(|range| !range.is_empty()) {
        Some(range) => handle.set_decorations(
            MATCH_LAYER,
            vec![zgui_editor::Decoration {
                range,
                kind: zgui_editor::DecorationKind::Background(
                    zgui_editor::decoration::Paint::Property("editor-search-current".into()),
                ),
            }],
        ),
        None => handle.clear_decorations(MATCH_LAYER),
    }
}

/// How much of a file is worth previewing.
///
/// Only the head is ever on the screen, and reading a hundred megabytes to show forty lines of it
/// is a stall for nothing.
const PREVIEW_HEAD: u64 = 256 * 1024;
