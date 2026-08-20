//! The git panel, wired into the editor.
//!
//! The panel itself is [`zdt_gitui`] and has never heard of a workspace. This is the other half:
//! what "say", "open" and "answer a key" mean when the panel is inside this editor.

use std::path::Path;
use std::rc::Rc;

use zgui::vocab::{KeyEvent, Modifiers};

use crate::notify::Announcer;
use crate::vim::Vim;
use crate::workspace::{BufferKind, Workspace};

/// The panel, for this workspace.
///
/// Built where the rest of the services are, and never on demand. What it announces through is
/// passed in, because a context read after an await is gone and because the panel outlives any
/// one window. See `tests/context.rs`.
#[must_use]
pub fn panel(
    workspace: Workspace,
    vim: Vim,
    announcer: Announcer,
    status: crate::git::Status,
) -> zdt_gitui::GitUi {
    // The tooling root, because a repository encloses the directory somebody opened as often as
    // it is that directory.
    let root = workspace.project().tooling_root().to_path_buf();
    let host = Rc::new(Editor {
        vim,
        announcer,
        workspace,
        status,
    });
    zdt_gitui::GitUi::new(&root, host)
}

/// This editor, as the panel sees it.
struct Editor {
    workspace: Workspace,
    vim: Vim,
    /// Where anything that went wrong is announced.
    announcer: Announcer,
    /// What the file tree draws its marks from, so staging in the panel shows up in the tree.
    status: crate::git::Status,
}

impl zdt_gitui::Host for Editor {
    fn say(&self, said: &str) {
        self.workspace.say(said);
    }

    fn complain(&self, said: &str) {
        // A failure is news, so it goes to the stack that waits to be read, and waits there for a
        // window when none is open.
        self.announcer.fail("git", Some(said.to_owned()));
    }

    fn open(&self, path: &Path) {
        crate::files::open(&self.workspace, path);
    }

    fn open_as_tab(&self) {
        self.workspace.open_panel(BufferKind::Git);
    }

    /// Says the panel has just taken the keyboard, which a click on one of its rows does.
    ///
    /// Only meaningful for the tab: the modal is an overlay and already has the keys whenever it
    /// is up.
    fn took_keyboard(&self) {
        if let Some(window) = self.tab_window() {
            self.workspace.focus().enter_window(window);
        }
    }

    /// Whether the keyboard is in the panel, in either of its two presentations.
    fn has_keyboard(&self) -> bool {
        match self.workspace.focus().current() {
            crate::focus::Focus::Overlay(crate::focus::Overlay::GitModal) => true,
            crate::focus::Focus::Window(window) => self.shows_git(window),
            _ => false,
        }
    }

    fn key(&self, event: &KeyEvent, modifiers: Modifiers) -> bool {
        crate::keys::chord_of(event, modifiers)
            .is_some_and(|chord| self.vim.key_in_region(chord, zdt_gitui::REGION))
    }

    /// Reads the tree's marks again, so staging in the panel shows up in the file tree.
    fn changed(&self) {
        self.status.refresh_soon();
    }
}

impl Editor {
    /// The window the panel's tab is in, when the current pane is showing one.
    fn tab_window(&self) -> Option<crate::workspace::WindowId> {
        let window = self.workspace.focused_untracked();
        self.shows_git(window).then_some(window)
    }

    /// Whether `window` is showing the panel as a tab. Tracked.
    fn shows_git(&self, window: crate::workspace::WindowId) -> bool {
        self.workspace
            .window(window)
            .and_then(|state| state.current)
            .and_then(|buffer| self.workspace.buffer(buffer))
            .is_some_and(|buffer| matches!(buffer.kind, BufferKind::Git))
    }
}
