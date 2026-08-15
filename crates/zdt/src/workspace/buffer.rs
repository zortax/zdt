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
    /// The revision it was last written at. Different from `revision` means unsaved changes.
    pub saved_revision: RwSignal<u64, LocalStorage>,
}

impl Buffer {
    /// A text buffer over `document`, from `path` when it came from one.
    pub fn text(id: BufferId, path: Option<PathBuf>, document: zgui_editor::Document) -> Self {
        let file_type = path
            .as_deref()
            .map(zdt_core::language::of)
            .unwrap_or(zdt_core::language::UNKNOWN);
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
        }
    }

    /// The document, when this is text.
    pub fn document(&self) -> Option<&zgui_editor::Document> {
        match &self.kind {
            BufferKind::Text { document } => Some(document),
            BufferKind::Terminal { .. } => None,
        }
    }

    /// Whether this is a terminal.
    pub fn is_terminal(&self) -> bool {
        matches!(self.kind, BufferKind::Terminal { .. })
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

    /// Whether the text has changed since it was last written.
    ///
    /// Tracked: a buffer tab reading this wakes when the text moves and at no other time.
    pub fn is_dirty(&self) -> bool {
        self.revision.get() != self.saved_revision.get()
    }

    /// Marks the buffer as written at the revision it is at now.
    pub fn mark_saved(&self) {
        self.saved_revision.set(self.revision.get_untracked());
    }

    /// Whether this buffer is the file at `path`.
    pub fn is_at(&self, path: &Path) -> bool {
        self.path.as_deref() == Some(path)
    }
}
