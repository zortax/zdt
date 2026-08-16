//! The git panel, wired into the editor.
//!
//! The panel itself is [`zdt_gitui`] and has never heard of a workspace. This is the other half:
//! what "say", "open" and "answer a key" mean when the panel is inside this editor.

use std::path::Path;
use std::rc::Rc;

use zgui::vocab::{KeyEvent, Modifiers};

use crate::notify::Notify;
use crate::vim::Vim;
use crate::workspace::{BufferKind, Workspace};

/// The panel, for this workspace.
///
/// Built where the rest of the services are, and never on demand. `Notify` is read from the
/// context here, and a context read after an await is gone. See `tests/context.rs`.
#[must_use]
pub fn panel(workspace: Workspace) -> zdt_gitui::GitUi {
    let root = workspace.project().root().to_path_buf();
    let host = Rc::new(Editor {
        vim: crate::vim::use_vim(),
        notify: crate::notify::use_notify(),
        workspace,
    });
    zdt_gitui::GitUi::new(&root, host)
}

/// This editor, as the panel sees it.
struct Editor {
    workspace: Workspace,
    vim: Vim,
    /// Where anything that went wrong is announced. `None` in a test with no toaster over it.
    notify: Option<Notify>,
}

impl zdt_gitui::Host for Editor {
    fn say(&self, said: &str) {
        self.workspace.say(said);
    }

    fn complain(&self, said: &str) {
        // A failure is news, so it goes to the stack that waits to be read. The status line is the
        // fallback for a window that has no toaster.
        match self.notify.as_ref() {
            Some(notify) => notify.fail("git", Some(said.to_owned())),
            None => self.workspace.complain(said),
        }
    }

    fn open(&self, path: &Path) {
        crate::files::open(&self.workspace, path);
    }

    fn open_as_tab(&self) {
        self.workspace.open_panel(BufferKind::Git);
    }

    fn release_keyboard(&self) {
        self.workspace.focus_editor();
    }

    fn key(&self, event: &KeyEvent, modifiers: Modifiers) -> bool {
        crate::keys::chord_of(event, modifiers)
            .is_some_and(|chord| self.vim.key_in_region(chord, zdt_gitui::REGION))
    }

    fn is_in_front(&self) -> bool {
        // Tracked, so that the panel takes the keyboard when its tab becomes the current one.
        self.workspace
            .current_buffer()
            .is_some_and(|buffer| matches!(buffer.kind, BufferKind::Git))
    }
}
