//! Opening and saving, from the interface's side.
//!
//! The reading and the writing are [`zdt_core::fs`]'s and happen on a worker. What is here is the
//! part that has to be on the interface thread: deciding whether the file is already open, making
//! the buffer, and saying what went wrong.

use std::path::{Path, PathBuf};

use zgui::reactive::prelude::*;
use zgui::task::{background, blocking};

use crate::workspace::{BufferId, Workspace};

/// Opens `path`, showing it in the focused window.
///
/// Returns immediately: the file is read on a worker and the buffer appears when it arrives. A
/// file already open is shown at once and never re-read, which is what stops `<Leader>ff` onto
/// something on the buffer line from throwing away its undo history.
pub fn open(workspace: &Workspace, path: impl Into<PathBuf>) {
    open_at(workspace, path, None);
}

/// The same, and puts the caret on `line` — counting from one — when there is one.
///
/// What a grep hit and a jump to a definition both come to.
pub fn open_at(workspace: &Workspace, path: impl Into<PathBuf>, line: Option<u64>) {
    let path = path.into();
    if let Some(existing) = workspace.find_path(&path) {
        workspace.show(existing);
        go_to_line(workspace, existing, line);
        return;
    }

    let workspace = workspace.clone();
    // Detached: opening a file is often what closes the picker that asked for it, and a read
    // belonging to the picker would be cancelled before the buffer ever appeared.
    crate::task::detached(async move {
        let reading = path.clone();
        let loaded = blocking(move || zdt_core::fs::load(&reading)).await;

        match loaded {
            Ok(file) => {
                let document = zgui_editor::Document::new(&file.text);
                let id = workspace.open_document(Some(path.clone()), document);
                go_to_line(&workspace, id, line);
                if let Some(buffer) = workspace.buffer_untracked(id) {
                    // The encoding and the line ending are the file's, and have to be put back
                    // exactly when it is written. They are read once, here.
                    let updated = crate::workspace::Buffer {
                        encoding: file.encoding,
                        line_ending: file.line_ending,
                        lossy: file.lossy,
                        ..buffer
                    };
                    workspace.replace_buffer(updated);
                }
                if file.lossy {
                    workspace.complain(format!(
                        "{}: some bytes are not valid text and were replaced",
                        path.display()
                    ));
                } else {
                    workspace.hush();
                }
            }
            Err(error) => workspace.complain(error.to_string()),
        }
    });
}

/// Puts the caret at the start of `line` in `buffer`, once there is an editor showing it.
///
/// `line` counts from one, the way a grep hit and an error message both do.
pub fn go_to(workspace: &Workspace, buffer: BufferId, line: u64) {
    go_to_line(workspace, buffer, Some(line));
}

/// The same, for a line that may not be there.
///
/// From a timer, because a buffer that has just been opened has no mounted editor yet: the view is
/// built on the next frame, and there is nothing to scroll until it is.
fn go_to_line(workspace: &Workspace, buffer: BufferId, line: Option<u64>) {
    let Some(line) = line.filter(|line| *line > 0) else {
        return;
    };
    let Some(timers) = zgui::view::time::Timers::current() else {
        return;
    };
    let workspace = workspace.clone();
    let handle = timers.set_timeout(std::time::Duration::ZERO, move || {
        let window = workspace.focused_untracked();
        let Some(editor) = workspace.handle_for(window, buffer) else {
            return;
        };
        let wanted = (line - 1) as usize;
        let at = editor.query(|snapshot| {
            let rope = snapshot.rope();
            let line = wanted.min(rope.len_lines().saturating_sub(1));
            rope.char_to_byte(rope.line_to_char(line))
        });
        editor.command(zgui_editor::Command::SetSelections {
            selections: vec![zgui_editor::Selection::caret(at)],
            primary: 0,
        });
        // Centred rather than merely visible: a hit at the bottom of the screen with nothing
        // under it reads as the end of the file even when it is not.
        editor.command(zgui_editor::Command::Scroll(
            zgui_editor::ScrollCmd::CursorCenter,
        ));
    });
    // Held nowhere: dropping a timer handle cancels it, and this one has to fire.
    std::mem::forget(handle);
}

/// Opens `path` when it is a file, or says why it cannot.
///
/// What the command line hands over: a directory is the project rather than a buffer, and a path
/// that is not there yet is a new file rather than a mistake.
pub fn open_argument(workspace: &Workspace, path: &Path) {
    if path.is_dir() {
        return;
    }
    if path.exists() {
        open(workspace, path);
    } else {
        // A file that is not there yet: an empty buffer that knows where it will be written.
        workspace.open_document(Some(path.to_path_buf()), zgui_editor::Document::new(""));
    }
}

/// Writes `buffer` back to where it came from.
///
/// Says so in the status line either way. A buffer with no path cannot be written and says that
/// instead of failing silently.
pub fn save(workspace: &Workspace, buffer: BufferId) {
    let Some(entry) = workspace.buffer_untracked(buffer) else {
        return;
    };
    let Some(path) = entry.path.clone() else {
        workspace.complain("no file name; use :w <path>");
        return;
    };
    let Some(document) = entry.document().cloned() else {
        return;
    };

    save_as(workspace, buffer, path, document);
}

/// Writes `document` to `path` and marks `buffer` saved at the revision that was written.
pub fn save_as(
    workspace: &Workspace,
    buffer: BufferId,
    path: PathBuf,
    document: zgui_editor::Document,
) {
    let Some(entry) = workspace.buffer_untracked(buffer) else {
        return;
    };

    // The text and the revision are taken together, on this thread, so that what is written and
    // what is marked as written are the same thing however much is typed while the write runs.
    let text = document.text();
    let revision = document.revision();
    let (encoding, line_ending) = (entry.encoding, entry.line_ending);

    let workspace = workspace.clone();
    // Detached: `:wq` closes the window that asked, and a write cancelled half way is a file lost.
    crate::task::detached(async move {
        let writing = path.clone();
        let written =
            background(async move { zdt_core::fs::save(&writing, &text, encoding, line_ending) })
                .await;

        match written {
            Ok(()) => {
                if let Some(entry) = workspace.buffer_untracked(buffer) {
                    entry.saved_revision.set(revision);
                }
                workspace.say(format!("{} written", path.display()));
            }
            Err(error) => workspace.complain(error.to_string()),
        }
    });
}
