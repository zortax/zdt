//! Tree-sitter colours for text that is in no editor.
//!
//! The editor colours its buffers itself. Everything else that shows code — a diff line, a
//! preview a surface draws on its own — comes here. One call parses a whole text and answers
//! every line's spans; a shared cache keyed by content makes asking again free, so a diff that
//! is recomputed on every git event re-parses only the files that changed.
//!
//! Colours stay in the cascade. Each capture resolves to a class over a `--syntax-*` custom
//! property, the same vocabulary the editor reads, so a theme change recolours every span with
//! no work here.

mod cache;
mod vocabulary;
mod word;

use std::sync::Arc;

use smallvec::SmallVec;
use zgui_editor::syntax::LineSpan;

pub use crate::vocabulary::STYLE;
pub use crate::word::line_view;

/// Every line of one text, highlighted and resolved to classes.
#[derive(Debug, Default)]
pub struct Highlights {
    /// The class of each capture, by the index a span carries.
    classes: Vec<Option<&'static str>>,
    /// One entry per line: spans in line-local byte offsets, sorted by start and then end.
    lines: Vec<SmallVec<[LineSpan; 8]>>,
}

impl Highlights {
    /// The spans of one line, by its number counting from one.
    #[must_use]
    pub fn spans(&self, number: u32) -> &[LineSpan] {
        number
            .checked_sub(1)
            .and_then(|index| self.lines.get(index as usize))
            .map_or(&[], |spans| spans.as_slice())
    }

    /// The class a capture resolves to, when the vocabulary holds one for it.
    #[must_use]
    pub fn class(&self, capture: u16) -> Option<&'static str> {
        self.classes.get(capture as usize).copied().flatten()
    }
}

/// The whole of `text` highlighted as whatever `path` says it is.
///
/// Parses on a cache miss, so this is for a worker thread. `None` when no loaded grammar claims
/// the path, which leaves the text plain.
#[must_use]
pub fn of(path: &std::path::Path, text: &Arc<str>) -> Option<Arc<Highlights>> {
    let name = zdt_core::language::of(path).language?;
    cache::shared().of(name, text)
}

/// Both sides of one file's diff, highlighted.
#[derive(Clone, Debug, Default)]
pub struct DiffMarks {
    /// The old text's colours, when the old side is text a grammar claims.
    pub old: Option<Arc<Highlights>>,
    /// The new text's colours. See `old`.
    pub new: Option<Arc<Highlights>>,
}

impl DiffMarks {
    /// The colours for one diff line: which side its text was read from, and its line number
    /// there.
    #[must_use]
    pub fn line(
        &self,
        kind: zdt_git::LineKind,
        old: Option<u32>,
        new: Option<u32>,
    ) -> Option<(&Arc<Highlights>, u32)> {
        let old = || self.old.as_ref().zip(old);
        let new_side = self.new.as_ref().zip(new);
        match kind {
            zdt_git::LineKind::Added => new_side,
            zdt_git::LineKind::Removed => old(),
            zdt_git::LineKind::Context => new_side.or_else(old),
        }
    }
}

/// The colours of one file's diff, both sides from the exact texts the diff was cut from.
#[must_use]
pub fn marks_of(diff: &zdt_git::FileDiff) -> DiffMarks {
    let path = std::path::Path::new(&diff.path);
    DiffMarks {
        old: diff.old_text.as_ref().and_then(|text| of(path, text)),
        new: diff.new_text.as_ref().and_then(|text| of(path, text)),
    }
}
