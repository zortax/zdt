//! What the modal layer is driving.
//!
//! The engine is pure: it reads a rope, some carets and a view, and answers a list of things to
//! do. Two things in zdt can be that — a document in an editor, and what a terminal holds — so
//! the seam names both and every key path goes through one of them.
//!
//! Borrowed, because a surface is what one keystroke is about. Nothing keeps one.

use zdt_vim::effect::{Context, Owner};
use zgui_editor::EditorHandle;

use crate::terminals::normal::Scrollback;

/// Which region's keys a terminal reads in front of the base map.
///
/// The same name in terminal mode and in terminal-normal mode: one file holds the way out of the
/// first and the ways back into it.
pub const TERMINAL: &str = "terminal";

/// What the modal layer is driving.
#[derive(Clone, Copy)]
pub enum Surface<'a> {
    /// A document in an editor.
    Editor(&'a EditorHandle),
    /// What a terminal holds, which nothing may edit.
    Terminal(&'a Scrollback),
}

impl<'a> Surface<'a> {
    /// Reads what the engine needs to decide.
    ///
    /// `owner` is which buffer this is, as the engine names them. The engine never interprets it
    /// and hands it back with a mark or a jump.
    pub fn query<R>(&self, owner: Owner, read: impl FnOnce(&Context<'_>) -> R) -> R {
        match self {
            Self::Editor(handle) => handle.query(|snapshot| {
                let selections: Vec<zdt_vim::effect::Selection> = snapshot
                    .selections()
                    .iter()
                    .map(|selection| {
                        zdt_vim::effect::Selection::new(selection.anchor, selection.head)
                    })
                    .collect();
                let visible = snapshot.visible_lines();
                read(&Context {
                    rope: snapshot.rope(),
                    selections: &selections,
                    view: zdt_vim::motion::View {
                        top_line: visible.start,
                        height: visible.len().max(1),
                    },
                    owner,
                })
            }),
            Self::Terminal(scrollback) => scrollback.query(owner, read),
        }
    }

    /// Whose keys are read in front of the base map.
    #[must_use]
    pub fn region(&self) -> Option<&'static str> {
        match self {
            Self::Editor(_) => None,
            Self::Terminal(_) => Some(TERMINAL),
        }
    }

    /// The editor behind it, when there is one.
    #[must_use]
    pub fn editor(&self) -> Option<&'a EditorHandle> {
        match self {
            Self::Editor(handle) => Some(handle),
            Self::Terminal(_) => None,
        }
    }

    /// What a terminal it is drawing on, when it is drawing on one.
    #[must_use]
    pub fn scrollback(&self) -> Option<&'a Scrollback> {
        match self {
            Self::Editor(_) => None,
            Self::Terminal(scrollback) => Some(scrollback),
        }
    }
}
