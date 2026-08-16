//! What one open thing is.
//!
//! A buffer is a document plus everything about it that is not text: where it came from, how it is
//! spelled on disk, whether it has been saved since it was last changed. The text itself belongs
//! to [`zgui_editor::Document`], which is what lets the same buffer be open in two windows.

use std::path::{Path, PathBuf};

use zdt_core::language::FileType;
use zdt_core::{Encoding, LineEnding, Project};
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

slotmap::new_key_type! {
    /// Which buffer this is.
    pub struct BufferId;
}

/// What kind of thing a buffer holds.
#[derive(Clone)]
pub enum BufferKind {
    /// Text, in a document one or more editors can show.
    Text {
        /// The text, its history and its options.
        document: zgui_editor::Document,
    },
    /// A terminal, in its own emulator.
    ///
    /// Held here so that a terminal is a buffer like any other — it appears on the buffer line,
    /// `]b` walks onto it, and `<Leader>c` closes it.
    Terminal {
        /// What the program running in it calls itself, once it says.
        title: RwSignal<Option<String>, LocalStorage>,
    },
    /// The settings, as a page.
    ///
    /// A buffer rather than a modal so that it is a tab like any other: `]b` walks onto it,
    /// `<Leader>c` closes it, and it can be put in a split beside the file whose behaviour is
    /// being changed — which is the whole reason to want it as a tab.
    Settings,
    /// The git panel, as a page.
    Git,
}

/// One open buffer.
#[derive(Clone)]
pub struct Buffer {
    /// Which buffer this is.
    pub id: BufferId,
    /// Where it came from, when it came from anywhere.
    pub path: Option<PathBuf>,
    /// What it holds.
    pub kind: BufferKind,
    /// What kind of file it is, for its glyph and its grammar.
    pub file_type: FileType,
    /// How it is spelled on disk.
    pub encoding: Encoding,
    /// What it breaks its lines with on disk.
    pub line_ending: LineEnding,
    /// Whether bytes had to be replaced to read it.
    ///
    /// A buffer read this way must not be written back without being asked twice: saving would
    /// make the damage permanent.
    pub lossy: bool,
    /// The revision the text is at, as the views report it.
    pub revision: RwSignal<u64, LocalStorage>,
    /// The revision it was last written at.
    pub saved_revision: RwSignal<u64, LocalStorage>,
    /// What was written, as a length and a hash.
    ///
    /// The revision alone cannot answer whether a buffer is dirty: undoing back to the text that
    /// is on disk produces a *new* revision, not the old one, so a buffer edited and then undone
    /// would keep its mark for ever. What the mark means is "this differs from the file", and
    /// that is a question about the text.
    pub saved_text: RwSignal<Fingerprint, LocalStorage>,
    /// Whether the text differs from what was written, worked out when the text moves.
    ///
    /// Held rather than computed on read, because the buffer line reads it every frame and the
    /// answer costs a hash of the file.
    pub dirty: RwSignal<bool, LocalStorage>,
}

/// Enough of a text to tell it apart from another, cheaply.
///
/// A length and a hash. The length is checked first and rules out almost every case without
/// hashing anything: a buffer somebody has typed into is a different length nearly always.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Fingerprint {
    /// How many bytes it was.
    pub len: usize,
    /// What it hashed to.
    pub hash: u64,
}

impl Fingerprint {
    /// The fingerprint of `rope`.
    #[must_use]
    pub fn of(rope: &ropey::Rope) -> Self {
        use std::hash::{Hash, Hasher};

        let mut hasher = rustc_hash::FxHasher::default();
        for chunk in rope.chunks() {
            chunk.hash(&mut hasher);
        }
        Self {
            len: rope.len_bytes(),
            hash: hasher.finish(),
        }
    }

    /// Whether `rope` is what this was taken from.
    ///
    /// The length first: a mismatch there is an answer without hashing a megabyte.
    #[must_use]
    pub fn matches(&self, rope: &ropey::Rope) -> bool {
        rope.len_bytes() == self.len && Self::of(rope) == *self
    }
}

impl Buffer {
    /// A text buffer over `document`, from `path` when it came from one.
    pub fn text(id: BufferId, path: Option<PathBuf>, document: zgui_editor::Document) -> Self {
        let file_type = path
            .as_deref()
            .map(zdt_core::language::of)
            .unwrap_or(zdt_core::language::UNKNOWN);
        // A buffer opens holding exactly what was read, so what it opens with *is* what is on
        // disk — and a file that is not there yet is an empty buffer whose empty text matches.
        let saved = Fingerprint::of(&document.rope());
        Self {
            id,
            path,
            kind: BufferKind::Text { document },
            file_type,
            encoding: Encoding::default(),
            line_ending: LineEnding::default(),
            lossy: false,
            revision: RwSignal::new_local(0),
            saved_revision: RwSignal::new_local(0),
            saved_text: RwSignal::new_local(saved),
            dirty: RwSignal::new_local(false),
        }
    }

    /// A terminal buffer called `name`.
    pub fn terminal(id: BufferId, name: &str) -> Self {
        Self {
            id,
            path: None,
            kind: BufferKind::Terminal {
                title: RwSignal::new_local(Some(name.to_owned())),
            },
            file_type: zdt_core::language::TERMINAL,
            encoding: Encoding::default(),
            line_ending: LineEnding::default(),
            lossy: false,
            revision: RwSignal::new_local(0),
            saved_revision: RwSignal::new_local(0),
            saved_text: RwSignal::new_local(Fingerprint::default()),
            dirty: RwSignal::new_local(false),
        }
    }

    /// A panel buffer: the settings, or the git page.
    ///
    /// No path, no text, nothing to save. What makes it a buffer at all is that the buffer line,
    /// the window layout and every key that walks between tabs already work on buffers, and a
    /// panel that was none of those things would need all three written again.
    pub fn panel(id: BufferId, kind: BufferKind) -> Self {
        let file_type = match kind {
            BufferKind::Git => zdt_core::language::GIT,
            _ => zdt_core::language::SETTINGS,
        };
        Self {
            id,
            path: None,
            kind,
            file_type,
            encoding: Encoding::default(),
            line_ending: LineEnding::default(),
            lossy: false,
            revision: RwSignal::new_local(0),
            saved_revision: RwSignal::new_local(0),
            saved_text: RwSignal::new_local(Fingerprint::default()),
            dirty: RwSignal::new_local(false),
        }
    }

    /// The document, when this is text.
    pub fn document(&self) -> Option<&zgui_editor::Document> {
        match &self.kind {
            BufferKind::Text { document } => Some(document),
            _ => None,
        }
    }

    /// Whether this is a terminal.
    pub fn is_terminal(&self) -> bool {
        matches!(self.kind, BufferKind::Terminal { .. })
    }

    /// Whether this is a panel rather than something being edited.
    ///
    /// What the things that only make sense over text ask before doing anything: saving, telling a
    /// language server, working out a diff.
    pub fn is_panel(&self) -> bool {
        matches!(self.kind, BufferKind::Settings | BufferKind::Git)
    }

    /// Which grammar highlights it.
    pub fn language(&self) -> Option<&'static str> {
        self.file_type.language
    }

    /// What the buffer line calls it.
    ///
    /// The file's name, or what it is instead of a file. Two open buffers with the same name are
    /// told apart by [`label_in`](Self::label_in), which the buffer line uses when it has to.
    pub fn name(&self) -> String {
        match (&self.path, &self.kind) {
            (Some(path), _) => path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned()),
            (None, BufferKind::Terminal { title }) => title
                .get_untracked()
                .unwrap_or_else(|| "terminal".to_owned()),
            (None, BufferKind::Settings) => "settings".to_owned(),
            (None, BufferKind::Git) => "git".to_owned(),
            (None, BufferKind::Text { .. }) => "[no name]".to_owned(),
        }
    }

    /// What a status line calls it: the path from the project root, or the plain name.
    pub fn label_in(&self, project: &Project) -> String {
        match &self.path {
            Some(path) => project.relative(path).into_owned(),
            None => self.name(),
        }
    }

    /// Whether the text differs from what is on disk.
    ///
    /// Tracked: a buffer tab reading this wakes when the answer changes and at no other time.
    pub fn is_dirty(&self) -> bool {
        self.dirty.get()
    }

    /// Works out whether it still differs, and remembers the answer.
    ///
    /// Called when the text moves. The revision is checked first — equal revisions cannot differ
    /// — and then the fingerprint, whose length check settles nearly every case for nothing.
    pub fn refresh_dirty(&self) {
        let Some(document) = self.document() else {
            return;
        };
        let revision = document.revision();
        self.revision.set(revision);

        let dirty = revision != self.saved_revision.get_untracked()
            && !self
                .saved_text
                .with_untracked(|saved| saved.matches(&document.rope()));
        if self.dirty.get_untracked() != dirty {
            self.dirty.set(dirty);
        }
    }

    /// Marks the buffer as written at the revision and text it is at now.
    pub fn mark_saved(&self) {
        self.saved_revision.set(self.revision.get_untracked());
        if let Some(document) = self.document() {
            self.saved_text.set(Fingerprint::of(&document.rope()));
        }
        if self.dirty.get_untracked() {
            self.dirty.set(false);
        }
    }

    /// Whether this buffer is the file at `path`.
    pub fn is_at(&self, path: &Path) -> bool {
        self.path.as_deref() == Some(path)
    }
}

#[cfg(test)]
mod tests {
    use super::Fingerprint;

    #[test]
    fn a_fingerprint_knows_its_own_text() {
        let rope = ropey::Rope::from_str("hello\nworld\n");
        let taken = Fingerprint::of(&rope);
        assert!(taken.matches(&rope));
    }

    #[test]
    fn text_that_changed_and_changed_back_matches_again() {
        // The whole reason a fingerprint is used rather than the revision: undoing produces a new
        // revision, never the old one, so only the text can answer whether anything differs.
        let saved = Fingerprint::of(&ropey::Rope::from_str("hello\n"));

        assert!(!saved.matches(&ropey::Rope::from_str("goodbye\n")));
        assert!(saved.matches(&ropey::Rope::from_str("hello\n")));
    }

    #[test]
    fn a_different_length_is_answered_without_hashing() {
        // Not observable from outside, which is the point: the length check is what makes this
        // cheap enough to run on every keystroke. What is observable is that it still answers.
        let saved = Fingerprint::of(&ropey::Rope::from_str("hello"));
        assert!(!saved.matches(&ropey::Rope::from_str("hello world")));
    }

    #[test]
    fn the_same_length_and_different_text_is_caught() {
        let saved = Fingerprint::of(&ropey::Rope::from_str("abcd"));
        assert!(!saved.matches(&ropey::Rope::from_str("abce")));
    }

    #[test]
    fn an_empty_text_is_its_own_fingerprint() {
        let empty = ropey::Rope::from_str("");
        assert!(Fingerprint::of(&empty).matches(&empty));
        assert!(!Fingerprint::of(&empty).matches(&ropey::Rope::from_str("x")));
    }
}
