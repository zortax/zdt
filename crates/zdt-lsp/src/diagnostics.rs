//! What the servers think is wrong.
//!
//! One store for every file and every server. Keyed by both, because two servers can have opinions
//! about one file and a new set from one of them must not throw away the other's — which is what a
//! store keyed by file alone would do the moment somebody ran a linter beside a type checker.
//!
//! Everything here is plain data. What to underline, what to put in the gutter and what to say in
//! the status line are the interface's business; this only says what is wrong and where.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lsp_types::{Diagnostic, DiagnosticSeverity};

/// How many of each kind there are.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Counts {
    /// Errors.
    pub errors: usize,
    /// Warnings.
    pub warnings: usize,
    /// Information.
    pub information: usize,
    /// Hints.
    pub hints: usize,
}

impl Counts {
    /// Whether there is nothing to report.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.errors == 0 && self.warnings == 0 && self.information == 0 && self.hints == 0
    }

    /// The worst thing in them, when there is anything.
    #[must_use]
    pub fn worst(&self) -> Option<DiagnosticSeverity> {
        if self.errors > 0 {
            Some(DiagnosticSeverity::ERROR)
        } else if self.warnings > 0 {
            Some(DiagnosticSeverity::WARNING)
        } else if self.information > 0 {
            Some(DiagnosticSeverity::INFORMATION)
        } else if self.hints > 0 {
            Some(DiagnosticSeverity::HINT)
        } else {
            None
        }
    }

    /// Adds one of `severity`.
    fn add(&mut self, severity: Option<DiagnosticSeverity>) {
        match severity.unwrap_or(DiagnosticSeverity::ERROR) {
            DiagnosticSeverity::WARNING => self.warnings += 1,
            DiagnosticSeverity::INFORMATION => self.information += 1,
            DiagnosticSeverity::HINT => self.hints += 1,
            // A server that says nothing about how bad it is means an error, per the protocol.
            _ => self.errors += 1,
        }
    }
}

impl std::ops::AddAssign for Counts {
    fn add_assign(&mut self, other: Self) {
        self.errors += other.errors;
        self.warnings += other.warnings;
        self.information += other.information;
        self.hints += other.hints;
    }
}

/// Everything every server has said.
#[derive(Debug, Default)]
pub struct Store {
    /// By file, then by the server that said it.
    by_file: BTreeMap<PathBuf, BTreeMap<String, Vec<Diagnostic>>>,
}

impl Store {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces what `server` says about `path`.
    ///
    /// Replacing rather than adding is the protocol's rule: a publish is the whole truth about
    /// that file from that server, and an empty one means it is happy now.
    pub fn set(&mut self, path: &Path, server: &str, diagnostics: Vec<Diagnostic>) {
        let per_server = self.by_file.entry(path.to_path_buf()).or_default();
        if diagnostics.is_empty() {
            per_server.remove(server);
            if per_server.is_empty() {
                self.by_file.remove(path);
            }
            return;
        }
        per_server.insert(server.to_owned(), diagnostics);
    }

    /// Forgets everything about `path`, which closing it does.
    pub fn forget(&mut self, path: &Path) {
        self.by_file.remove(path);
    }

    /// Forgets everything `server` said, which its going away does.
    pub fn forget_server(&mut self, server: &str) {
        self.by_file.retain(|_, per_server| {
            per_server.remove(server);
            !per_server.is_empty()
        });
    }

    /// Everything wrong with `path`, from every server, in document order.
    #[must_use]
    pub fn for_file(&self, path: &Path) -> Vec<Diagnostic> {
        let Some(per_server) = self.by_file.get(path) else {
            return Vec::new();
        };
        let mut found: Vec<Diagnostic> = per_server.values().flatten().cloned().collect();
        found.sort_by(|left, right| {
            (left.range.start.line, left.range.start.character)
                .cmp(&(right.range.start.line, right.range.start.character))
        });
        found
    }

    /// How many of each kind `path` has.
    #[must_use]
    pub fn counts(&self, path: &Path) -> Counts {
        let mut counts = Counts::default();
        if let Some(per_server) = self.by_file.get(path) {
            for diagnostic in per_server.values().flatten() {
                counts.add(diagnostic.severity);
            }
        }
        counts
    }

    /// How many of each kind there are altogether.
    #[must_use]
    pub fn total(&self) -> Counts {
        let mut counts = Counts::default();
        for per_server in self.by_file.values() {
            for diagnostic in per_server.values().flatten() {
                counts.add(diagnostic.severity);
            }
        }
        counts
    }

    /// Every file that has anything wrong with it.
    #[must_use]
    pub fn files(&self) -> Vec<PathBuf> {
        self.by_file.keys().cloned().collect()
    }

    /// The next diagnostic after `line` in `path`, wrapping to the first.
    ///
    /// What `]d` is. Wrapping rather than stopping, because a file with one error in it at the top
    /// is a file where `]d` should still reach it from the bottom.
    #[must_use]
    pub fn after(&self, path: &Path, line: u32) -> Option<Diagnostic> {
        let found = self.for_file(path);
        found
            .iter()
            .find(|one| one.range.start.line > line)
            .or_else(|| found.first())
            .cloned()
    }

    /// The one before `line`, wrapping to the last. What `[d` is.
    #[must_use]
    pub fn before(&self, path: &Path, line: u32) -> Option<Diagnostic> {
        let found = self.for_file(path);
        found
            .iter()
            .rev()
            .find(|one| one.range.start.line < line)
            .or_else(|| found.last())
            .cloned()
    }

    /// Everything on `line` in `path`, which is what a message popover shows.
    #[must_use]
    pub fn on_line(&self, path: &Path, line: u32) -> Vec<Diagnostic> {
        self.for_file(path)
            .into_iter()
            .filter(|one| one.range.start.line <= line && one.range.end.line >= line)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use lsp_types::{Position, Range};

    use super::*;

    fn at(line: u32, severity: DiagnosticSeverity, message: &str) -> Diagnostic {
        Diagnostic {
            range: Range {
                start: Position::new(line, 0),
                end: Position::new(line, 5),
            },
            severity: Some(severity),
            message: message.to_owned(),
            ..Diagnostic::default()
        }
    }

    fn file() -> PathBuf {
        PathBuf::from("/project/src/main.rs")
    }

    #[test]
    fn a_publish_replaces_what_that_server_said_before() {
        let mut store = Store::new();
        store.set(
            &file(),
            "rust-analyzer",
            vec![at(1, DiagnosticSeverity::ERROR, "one")],
        );
        store.set(
            &file(),
            "rust-analyzer",
            vec![at(2, DiagnosticSeverity::ERROR, "two")],
        );

        let found = store.for_file(&file());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].message, "two");
    }

    #[test]
    fn two_servers_do_not_overwrite_each_other() {
        let mut store = Store::new();
        store.set(
            &file(),
            "rust-analyzer",
            vec![at(1, DiagnosticSeverity::ERROR, "types")],
        );
        store.set(
            &file(),
            "clippy",
            vec![at(2, DiagnosticSeverity::WARNING, "style")],
        );

        assert_eq!(store.for_file(&file()).len(), 2);
        assert_eq!(
            store.counts(&file()),
            Counts {
                errors: 1,
                warnings: 1,
                ..Counts::default()
            }
        );
    }

    #[test]
    fn an_empty_publish_means_that_server_is_happy() {
        let mut store = Store::new();
        store.set(
            &file(),
            "rust-analyzer",
            vec![at(1, DiagnosticSeverity::ERROR, "one")],
        );
        store.set(
            &file(),
            "clippy",
            vec![at(2, DiagnosticSeverity::WARNING, "two")],
        );

        store.set(&file(), "rust-analyzer", Vec::new());
        let found = store.for_file(&file());
        assert_eq!(found.len(), 1, "the other server's is kept");
        assert_eq!(found[0].message, "two");
    }

    #[test]
    fn they_come_back_in_document_order() {
        let mut store = Store::new();
        store.set(&file(), "b", vec![at(9, DiagnosticSeverity::ERROR, "late")]);
        store.set(
            &file(),
            "a",
            vec![at(1, DiagnosticSeverity::ERROR, "early")],
        );

        let messages: Vec<String> = store
            .for_file(&file())
            .into_iter()
            .map(|one| one.message)
            .collect();
        assert_eq!(messages, vec!["early".to_owned(), "late".to_owned()]);
    }

    #[test]
    fn a_server_going_away_takes_its_opinions_with_it() {
        let mut store = Store::new();
        store.set(
            &file(),
            "rust-analyzer",
            vec![at(1, DiagnosticSeverity::ERROR, "one")],
        );
        store.set(
            &file(),
            "clippy",
            vec![at(2, DiagnosticSeverity::WARNING, "two")],
        );

        store.forget_server("rust-analyzer");
        assert_eq!(store.for_file(&file()).len(), 1);

        store.forget_server("clippy");
        assert!(store.for_file(&file()).is_empty());
        assert!(store.files().is_empty(), "and the file with it");
    }

    #[test]
    fn walking_wraps_at_the_ends() {
        let mut store = Store::new();
        store.set(
            &file(),
            "a",
            vec![
                at(1, DiagnosticSeverity::ERROR, "first"),
                at(5, DiagnosticSeverity::ERROR, "second"),
            ],
        );

        assert_eq!(store.after(&file(), 0).unwrap().message, "first");
        assert_eq!(store.after(&file(), 1).unwrap().message, "second");
        assert_eq!(
            store.after(&file(), 5).unwrap().message,
            "first",
            "past the last comes back to the first"
        );

        assert_eq!(store.before(&file(), 5).unwrap().message, "first");
        assert_eq!(
            store.before(&file(), 0).unwrap().message,
            "second",
            "and before the first is the last"
        );
    }

    #[test]
    fn walking_a_clean_file_finds_nothing() {
        let store = Store::new();
        assert!(store.after(&file(), 0).is_none());
        assert!(store.before(&file(), 0).is_none());
    }

    #[test]
    fn what_is_on_a_line_includes_what_spans_it() {
        let mut store = Store::new();
        let mut spanning = at(1, DiagnosticSeverity::ERROR, "block");
        spanning.range.end = Position::new(4, 0);
        store.set(
            &file(),
            "a",
            vec![spanning, at(9, DiagnosticSeverity::ERROR, "far")],
        );

        assert_eq!(store.on_line(&file(), 3).len(), 1);
        assert_eq!(store.on_line(&file(), 3)[0].message, "block");
        assert!(store.on_line(&file(), 7).is_empty());
    }

    #[test]
    fn a_diagnostic_with_no_severity_is_an_error() {
        let mut store = Store::new();
        let mut vague = at(1, DiagnosticSeverity::ERROR, "unsaid");
        vague.severity = None;
        store.set(&file(), "a", vec![vague]);

        assert_eq!(store.counts(&file()).errors, 1);
    }

    #[test]
    fn the_worst_thing_is_what_the_status_line_shows() {
        assert_eq!(Counts::default().worst(), None);
        assert_eq!(
            Counts {
                warnings: 3,
                hints: 9,
                ..Counts::default()
            }
            .worst(),
            Some(DiagnosticSeverity::WARNING)
        );
        assert_eq!(
            Counts {
                errors: 1,
                warnings: 99,
                ..Counts::default()
            }
            .worst(),
            Some(DiagnosticSeverity::ERROR)
        );
    }

    #[test]
    fn the_total_is_across_every_file() {
        let mut store = Store::new();
        store.set(&file(), "a", vec![at(1, DiagnosticSeverity::ERROR, "one")]);
        store.set(
            Path::new("/project/src/other.rs"),
            "a",
            vec![at(1, DiagnosticSeverity::WARNING, "two")],
        );

        assert_eq!(
            store.total(),
            Counts {
                errors: 1,
                warnings: 1,
                ..Counts::default()
            }
        );
        assert_eq!(store.files().len(), 2);
    }
}
