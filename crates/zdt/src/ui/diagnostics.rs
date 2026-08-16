//! Diagnostics on the screen.
//!
//! Three things, from one list: a squiggle under the text, a mark in the gutter, and a count in
//! the status line. All three are drawn from the same store, so they can never disagree.
//!
//! The squiggle and the mark go through the editor's decoration layers, which is what the P1 work
//! upstream was for: they are painted with the text rather than over it, so they scroll with it
//! and cost nothing per frame.

use std::path::PathBuf;

use lsp_types::DiagnosticSeverity;
use zgui_editor::decoration::{Decoration, DecorationKind, GutterMark, Paint, UnderlineStyle};

use crate::language::Language;
use crate::workspace::{BufferId, WindowId, Workspace};

/// The layer diagnostics are painted in.
///
/// Named, so that a search highlight and a git sign can be painted in their own without either
/// clearing the other.
const LAYER: &str = "lsp";

/// Keeps the editor showing `buffer`'s diagnostics in `window`.
///
/// Held for as long as the view is; dropping it stops the following, and the decorations go with
/// the editor.
pub fn follow(
    workspace: &Workspace,
    language: &Language,
    window: WindowId,
    buffer: BufferId,
) -> zgui::reactive::RenderEffect<()> {
    let (workspace, language) = (workspace.clone(), language.clone());
    zgui::reactive::RenderEffect::new(move |_| {
        // Read first, so this runs again whenever anything a server said has changed.
        let _ = language.revision();

        let Some(handle) = workspace.handle_for(window, buffer) else {
            return;
        };
        let Some(path) = workspace
            .buffer_untracked(buffer)
            .and_then(|entry| entry.path)
        else {
            return;
        };

        let found = language.diagnostics(&path);
        if found.is_empty() {
            handle.clear_decorations(LAYER);
            handle.clear_gutter_marks(LAYER);
            return;
        }

        // The ranges are the server's, in its own units, so they are converted against the text
        // the editor is holding rather than against what was sent.
        let encoding = language
            .client_for(&path)
            .map(|client| client.encoding)
            .unwrap_or_default();

        let (decorations, marks) = handle.query(|snapshot| {
            let rope = snapshot.rope();
            let mut decorations = Vec::new();
            let mut marks: Vec<GutterMark> = Vec::new();

            for one in &found {
                let range = zdt_lsp::convert::range_of(rope, one.range, encoding);
                // A server can name an empty range — a missing semicolon is at a point, not over
                // one — and an underline of no width is one nobody can see. One character wide is
                // the least that says where.
                let range = if range.is_empty() {
                    let end = (range.start + 1).min(rope.len_bytes());
                    range.start..end
                } else {
                    range
                };

                decorations.push(Decoration {
                    range: range.clone(),
                    kind: DecorationKind::Underline {
                        style: UnderlineStyle::Squiggly,
                        paint: Paint::Property(tint(one.severity).into()),
                    },
                });

                let line = rope.byte_to_line(range.start.min(rope.len_bytes()));
                // One mark per line, the worst thing on it: a gutter with two marks on one line
                // has nowhere to put the second.
                match marks.iter_mut().find(|mark| mark.line == line) {
                    Some(held) => {
                        if worse(one.severity, held.text.as_ref()) {
                            held.text = glyph(one.severity).into();
                            held.paint = Paint::Property(tint(one.severity).into());
                        }
                    }
                    None => marks.push(GutterMark {
                        line,
                        text: glyph(one.severity).into(),
                        paint: Paint::Property(tint(one.severity).into()),
                    }),
                }
            }
            (decorations, marks)
        });

        handle.set_decorations(LAYER, decorations);
        handle.set_gutter_marks(LAYER, marks);
    })
}

/// Which colour a severity is drawn in.
#[must_use]
pub const fn tint(severity: Option<DiagnosticSeverity>) -> &'static str {
    match severity {
        Some(DiagnosticSeverity::WARNING) => "zdt-diagnostic-warning",
        Some(DiagnosticSeverity::INFORMATION) => "zdt-diagnostic-information",
        Some(DiagnosticSeverity::HINT) => "zdt-diagnostic-hint",
        _ => "zdt-diagnostic-error",
    }
}

/// Which glyph a severity gets in the gutter.
#[must_use]
pub const fn glyph(severity: Option<DiagnosticSeverity>) -> &'static str {
    match severity {
        Some(DiagnosticSeverity::WARNING) => "\u{f071}",
        Some(DiagnosticSeverity::INFORMATION) => "\u{f05a}",
        Some(DiagnosticSeverity::HINT) => "\u{f0eb}",
        _ => "\u{f057}",
    }
}

/// Whether `severity` is worse than whatever glyph is already on the line.
fn worse(severity: Option<DiagnosticSeverity>, held: &str) -> bool {
    rank(severity) < rank_of_glyph(held)
}

/// How bad a severity is, lower being worse.
const fn rank(severity: Option<DiagnosticSeverity>) -> u8 {
    match severity {
        Some(DiagnosticSeverity::WARNING) => 1,
        Some(DiagnosticSeverity::INFORMATION) => 2,
        Some(DiagnosticSeverity::HINT) => 3,
        _ => 0,
    }
}

/// The same, read back off a mark that is already there.
fn rank_of_glyph(glyph: &str) -> u8 {
    match glyph {
        "\u{f071}" => 1,
        "\u{f05a}" => 2,
        "\u{f0eb}" => 3,
        _ => 0,
    }
}

/// One part of what the status line says: a glyph, a count, and which severity it is.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Part {
    /// The glyph and the number, together.
    pub text: String,
    /// Which severity it is, as the class the style sheet colours by.
    pub tone: &'static str,
}

/// What the status line says about a file, one part per severity.
///
/// Empty when there is nothing wrong, so the status line can leave the space to something else.
/// Each part carries its own tone rather than the whole thing being one colour: three errors and a
/// hint are two different pieces of news, and a status line that greys both says neither.
#[must_use]
pub fn summary(language: &Language, path: Option<&PathBuf>) -> Vec<Part> {
    let Some(path) = path else {
        return Vec::new();
    };
    let counts = language.counts(path);

    [
        (counts.errors, DiagnosticSeverity::ERROR, "error"),
        (counts.warnings, DiagnosticSeverity::WARNING, "warning"),
        (
            counts.information,
            DiagnosticSeverity::INFORMATION,
            "information",
        ),
        (counts.hints, DiagnosticSeverity::HINT, "hint"),
    ]
    .into_iter()
    .filter(|(count, _, _)| *count > 0)
    .map(|(count, severity, tone)| Part {
        // A thin space between: these glyphs are nearly a cell wide and a digit against one is
        // two marks that read as one.
        text: format!("{}\u{2009}{count}", glyph(Some(severity))),
        tone,
    })
    .collect()
}

/// The layer git signs are painted in, beside the diagnostics' own.
const GIT_LAYER: &str = "git";

/// Keeps the editor showing what git says about `buffer` in `window`.
///
/// Held for as long as the view is.
pub fn follow_git(
    workspace: &Workspace,
    git: &crate::git::Git,
    window: WindowId,
    buffer: BufferId,
) -> zgui::reactive::RenderEffect<()> {
    let (workspace, git) = (workspace.clone(), git.clone());
    zgui::reactive::RenderEffect::new(move |_| {
        // Read first, so this runs again whenever the diff has.
        let _ = git.revision();

        let Some(handle) = workspace.handle_for(window, buffer) else {
            return;
        };
        let Some(path) = workspace
            .buffer_untracked(buffer)
            .and_then(|entry| entry.path)
        else {
            return;
        };

        let hunks = git.hunks(&path);
        if hunks.is_empty() {
            handle.clear_gutter_marks(GIT_LAYER);
            return;
        }

        let marks: Vec<GutterMark> = hunks
            .iter()
            .flat_map(|hunk| {
                let (glyph, tint) = (
                    crate::git::glyph(hunk.change),
                    crate::git::tint(hunk.change),
                );
                hunk.lines().map(move |line| GutterMark {
                    line,
                    text: glyph.into(),
                    paint: Paint::Property(tint.into()),
                })
            })
            .collect();
        handle.set_gutter_marks(GIT_LAYER, marks);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_severity_has_its_own_colour_and_glyph() {
        let severities = [
            Some(DiagnosticSeverity::ERROR),
            Some(DiagnosticSeverity::WARNING),
            Some(DiagnosticSeverity::INFORMATION),
            Some(DiagnosticSeverity::HINT),
        ];
        let tints: Vec<&str> = severities.iter().map(|one| tint(*one)).collect();
        let glyphs: Vec<&str> = severities.iter().map(|one| glyph(*one)).collect();

        for (index, held) in tints.iter().enumerate() {
            assert!(
                !tints[index + 1..].contains(held),
                "two severities share {held}"
            );
        }
        for (index, held) in glyphs.iter().enumerate() {
            assert!(
                !glyphs[index + 1..].contains(held),
                "two severities share a glyph"
            );
        }
    }

    #[test]
    fn a_severity_that_was_not_said_is_an_error() {
        assert_eq!(tint(None), tint(Some(DiagnosticSeverity::ERROR)));
        assert_eq!(glyph(None), glyph(Some(DiagnosticSeverity::ERROR)));
    }

    #[test]
    fn the_worst_thing_on_a_line_is_what_the_gutter_shows() {
        let warning = glyph(Some(DiagnosticSeverity::WARNING));
        assert!(
            worse(Some(DiagnosticSeverity::ERROR), warning),
            "an error replaces a warning"
        );
        assert!(
            !worse(Some(DiagnosticSeverity::HINT), warning),
            "and a hint does not"
        );
    }
}
