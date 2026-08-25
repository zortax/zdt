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

/// The same, and puts the caret on `line` when there is one. `line` counts from one.
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
    zdt_view::detached(async move {
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
        // Centred, and not merely visible. A hit at the bottom of the screen with nothing under
        // it reads as the end of the file.
        editor.command(zgui_editor::Command::Scroll(
            zgui_editor::ScrollCmd::CursorCenter,
        ));
    });
    // Held nowhere: dropping a timer handle cancels it, and this one has to fire.
    std::mem::forget(handle);
}

/// Which open files to read again.
#[derive(Clone, Copy)]
pub enum Which<'a> {
    /// All of them.
    Everything,
    /// The ones at these paths, and no others.
    ///
    /// What a watch on the project asks for: a change on disk names the files it touched, and
    /// reading the twenty other buffers to find out they are unchanged is twenty reads for
    /// nothing.
    These(&'a rustc_hash::FxHashSet<PathBuf>),
}

/// Re-reads every open file whose bytes moved on disk under a clean buffer.
///
/// What a settled agent turn asks for: the agent writes files under the session, and a buffer
/// with no unsaved work follows the disk. A dirty buffer is left alone and keeps the editor's
/// save-time conflict handling. The gutter's signs are worked out again either way.
pub fn refresh_from_disk(workspace: &Workspace) {
    let git = zgui::reactive::use_local_context::<crate::git::Git>();
    if let Some(status) = zgui::reactive::use_local_context::<crate::git::Status>() {
        status.refresh_soon();
    }
    refresh(workspace, git.as_ref(), Which::Everything);
}

/// The same, against layers that are held rather than looked up.
///
/// What follows a watch. A context read from a timer answers nothing, so what the disk layer has
/// it hands over.
pub fn refresh(workspace: &Workspace, git: Option<&crate::git::Git>, which: Which<'_>) {
    for id in workspace.order_untracked() {
        let wanted = match which {
            Which::Everything => true,
            Which::These(paths) => workspace
                .buffer_untracked(id)
                .and_then(|entry| entry.path)
                .is_some_and(|path| paths.contains(&path)),
        };
        if wanted {
            reread(workspace, git, id);
        }
    }
}

/// Reads what is on disk into `buffer`, when the buffer has no unsaved work.
///
/// A dirty buffer is left alone: what somebody typed outweighs what a tool wrote, and the
/// save-time conflict handling is where the two are reconciled. A buffer read with replaced bytes
/// is left alone too, because writing one back is what would make the damage permanent.
fn reread(workspace: &Workspace, git: Option<&crate::git::Git>, buffer: BufferId) {
    let Some(entry) = workspace.buffer_untracked(buffer) else {
        return;
    };
    let Some(path) = entry.path.clone() else {
        return;
    };
    let Some(document) = entry.document().cloned() else {
        return;
    };
    if entry.is_dirty() || entry.lossy {
        return;
    }

    let workspace = workspace.clone();
    let git = git.cloned();
    zdt_view::detached(async move {
        let reading = path.clone();
        let Ok(file) = blocking(move || zdt_core::fs::load(&reading)).await else {
            return;
        };
        let held = document.text();
        if let Some(replacement) = narrowed(&held, &file.text) {
            // Checked again on this thread: a keystroke while the read ran makes the buffer
            // dirty, and a dirty buffer is never overwritten.
            let Some(entry) = workspace.buffer_untracked(buffer) else {
                return;
            };
            if entry.is_dirty() {
                return;
            }
            if document.apply(vec![replacement]) {
                entry.revision.set(document.revision());
                entry.mark_saved();
            }
        }
        if let Some(git) = &git {
            git.refresh_soon(buffer);
        }
    });
}

/// The one replacement that turns `held` into `found`, when the two differ.
///
/// The part that moved, and never the whole file. Every view maps its carets and its selections
/// through a change, so a replacement of the whole text takes every caret in it to the top: a file
/// a tool rewrote one line of would move the caret of somebody reading the other end of it.
fn narrowed(held: &str, found: &str) -> Option<(std::ops::Range<usize>, String)> {
    if held == found {
        return None;
    }

    let mut start = held
        .as_bytes()
        .iter()
        .zip(found.as_bytes())
        .take_while(|(one, two)| one == two)
        .count();
    while !held.is_char_boundary(start) || !found.is_char_boundary(start) {
        start -= 1;
    }

    let room = held.len().min(found.len()) - start;
    let mut tail = held
        .as_bytes()
        .iter()
        .rev()
        .zip(found.as_bytes().iter().rev())
        .take_while(|(one, two)| one == two)
        .count()
        .min(room);
    while !held.is_char_boundary(held.len() - tail) || !found.is_char_boundary(found.len() - tail) {
        tail -= 1;
    }

    Some((
        start..held.len() - tail,
        found[start..found.len() - tail].to_owned(),
    ))
}

/// Opens `path` when it is a file, or says why it cannot.
///
/// What the command line hands over. A directory is the project, and a path that is not there yet
/// is a new file.
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
    zdt_view::detached(async move {
        let writing = path.clone();
        let written =
            background(async move { zdt_core::fs::save(&writing, &text, encoding, line_ending) })
                .await;

        match written {
            Ok(()) => {
                if let Some(entry) = workspace.buffer_untracked(buffer) {
                    // What was written is now what is on disk, so the mark goes and the
                    // fingerprint an undo will be compared against is this text.
                    entry.revision.set(revision);
                    entry.mark_saved();
                }
                // The servers hear about the save, and the signs are worked out again from what
                // is now on disk.
                if let Some(language) =
                    zgui::reactive::use_local_context::<crate::language::Language>()
                {
                    language.saved(buffer);
                }
                if let Some(git) = zgui::reactive::use_local_context::<crate::git::Git>() {
                    git.refresh_soon(buffer);
                }
                // And the tree's marks, because a first write to a new file is what makes it
                // untracked rather than absent.
                if let Some(status) = zgui::reactive::use_local_context::<crate::git::Status>() {
                    status.refresh_soon();
                }
                workspace.say(format!("{} written", path.display()));
            }
            Err(error) => workspace.complain(error.to_string()),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::narrowed;

    /// `held` with the one replacement in it, which is what the document is given.
    fn applied(held: &str, found: &str) -> String {
        match narrowed(held, found) {
            Some((range, text)) => {
                let mut out = held.to_owned();
                out.replace_range(range, &text);
                out
            }
            None => held.to_owned(),
        }
    }

    #[test]
    fn a_text_that_did_not_move_is_no_replacement() {
        assert!(narrowed("one\ntwo\n", "one\ntwo\n").is_none());
    }

    #[test]
    fn only_the_part_that_moved_is_replaced() {
        // What keeps the caret of somebody reading the end of a file where they left it when a
        // tool rewrites a line at the top.
        let (range, text) = narrowed("one\ntwo\nthree\n", "one\nTWO\nthree\n").expect("it moved");
        assert_eq!(range, 4..7);
        assert_eq!(text, "TWO");
    }

    #[test]
    fn an_insertion_writes_a_line_and_a_removal_takes_one_back() {
        assert_eq!(
            applied("one\nthree\n", "one\ntwo\nthree\n"),
            "one\ntwo\nthree\n"
        );
        assert_eq!(applied("one\ntwo\nthree\n", "one\nthree\n"), "one\nthree\n");

        // Nothing outside the one line is touched either way.
        let (range, _) = narrowed("one\nthree\n", "one\ntwo\nthree\n").expect("it moved");
        assert!(range.start >= 4 && range.end <= 9);
    }

    #[test]
    fn a_replacement_lands_on_character_boundaries() {
        // Two characters that share a first byte. A range that cut between the bytes of one would
        // be a panic on the rope.
        let (range, text) = narrowed("a é b", "a è b").expect("it moved");
        assert!("a é b".is_char_boundary(range.start));
        assert!("a é b".is_char_boundary(range.end));
        assert_eq!(text, "è");
    }

    #[test]
    fn a_repeated_ending_is_never_counted_twice() {
        // The common head and the common tail must not overlap, or the range would run backwards.
        let (range, _) = narrowed("aa", "aaaa").expect("it moved");
        assert!(range.start <= range.end);
        assert_eq!(applied("aa", "aaaa"), "aaaa");
        assert_eq!(applied("aaaa", "aa"), "aa");
    }

    #[test]
    fn a_whole_file_that_changed_is_replaced_whole() {
        let (range, text) = narrowed("one\n", "two\n").expect("it moved");
        assert_eq!(range, 0..3);
        assert_eq!(text, "two");
    }
}
