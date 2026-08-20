//! Asking for a name, beside the row it is about.
//!
//! Making a file, renaming one, moving one and confirming a removal are all one line of text and
//! something to do with it. The shared [`Prompt`](crate::prompt::Prompt) can ask any of them, but
//! it opens in the middle of the window: the eye leaves the row, types a name, and comes back to
//! find out where the file went.
//!
//! So the tree has its own. One line, no title, opened under the row it is about or at the point
//! a pointer went down. What it is asking is an [`About`] rather than a sentence, because a field
//! that sits on the thing it is about has already said which one that is.

pub mod view;

use std::rc::Rc;

use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

/// What a field is asking for.
///
/// Decides the outline beside it and what a reader is told. The starting text and what to do with
/// the answer are data, and stay data.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum About {
    /// A name for a new file.
    NewFile,
    /// A name for a new directory.
    NewDirectory,
    /// A new name for the row the caret is on.
    Rename,
    /// A new path for it.
    Move,
    /// Whether to remove what is picked out.
    Delete,
}

impl About {
    /// The outline beside the field.
    #[must_use]
    pub const fn icon(self) -> &'static str {
        use zdt_icons as icons;
        match self {
            Self::NewFile => icons::FILE_PLUS,
            Self::NewDirectory => icons::FOLDER_PLUS,
            Self::Rename => icons::PENCIL,
            Self::Move => icons::REPLACE,
            Self::Delete => icons::TRASH,
        }
    }

    /// What a reader is told the field is for.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::NewFile => "New file",
            Self::NewDirectory => "New directory",
            Self::Rename => "New name",
            Self::Move => "New path",
            Self::Delete => "Delete? y or n",
        }
    }
}

/// Where a field opens.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum At {
    /// On a row, which is where a key starts one.
    Row(usize),
    /// Where the pointer was, which is where the menu starts one.
    Pointer(f32, f32),
}

/// A question waiting for an answer.
#[derive(Clone)]
pub struct Asking {
    /// What is being asked.
    pub about: About,
    /// What the field opens holding.
    pub start: String,
    /// Where it opens.
    pub at: At,
    /// What to do with the answer. A cancelled field calls nothing.
    pub answer: Rc<dyn Fn(&str)>,
}

impl PartialEq for Asking {
    fn eq(&self, other: &Self) -> bool {
        // The callback is not comparable, and two fields asking the same thing in the same place
        // are the same field as far as anything drawing one is concerned.
        self.about == other.about && self.start == other.start && self.at == other.at
    }
}

/// The tree's inline field.
#[derive(Clone, Copy)]
pub struct Field {
    asking: RwSignal<Option<Asking>, LocalStorage>,
}

impl Field {
    /// A field with nothing being asked.
    #[must_use]
    pub fn new() -> Self {
        Self {
            asking: RwSignal::new_local(None),
        }
    }

    /// Asks for a line of text, at `at`.
    pub fn ask(
        &self,
        about: About,
        start: impl Into<String>,
        at: At,
        answer: impl Fn(&str) + 'static,
    ) {
        self.asking.set(Some(Asking {
            about,
            start: start.into(),
            at,
            answer: Rc::new(answer),
        }));
    }

    /// What is being asked, if anything. Tracked.
    #[must_use]
    pub fn asking(&self) -> Option<Asking> {
        self.asking.get()
    }

    /// Whether something is being asked. Tracked.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.asking.with(Option::is_some)
    }

    /// Answers, and closes.
    ///
    /// An empty answer answers nothing: somebody who cleared the field and pressed return meant to
    /// get out of it.
    pub fn submit(&self, text: &str) {
        let Some(asking) = self.asking.get_untracked() else {
            return;
        };
        self.asking.set(None);
        let text = text.trim();
        if !text.is_empty() {
            (asking.answer)(text);
        }
    }

    /// Closes without answering.
    pub fn cancel(&self) {
        if self.asking.with_untracked(Option::is_some) {
            self.asking.set(None);
        }
    }
}

impl Default for Field {
    fn default() -> Self {
        Self::new()
    }
}

/// Puts the field where every component can find it.
pub fn provide(field: Field) {
    zgui::reactive::provide_local_context(field);
}

/// It, from inside a component.
///
/// # Panics
///
/// If none was provided above this component, which is a wiring mistake.
#[must_use]
pub fn use_field() -> Field {
    zgui::reactive::use_local_context::<Field>().expect("a tree field is provided at the root")
}
