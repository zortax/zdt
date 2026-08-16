//! Saving a session, and restoring one.

use crate::settings::Settings;
use crate::workspace::Workspace;
use zgui_editor::EditorHandle;

/// The sessions.
///
/// A session is the files that were open and where the caret was in each, kept under the project's
/// own name so that "the session for this project" needs nothing remembered.
pub(super) fn run(workspace: &Workspace, leaf: &str, handle: Option<&EditorHandle>) {
    use crate::session::{self, Entry, Session};

    let paths = zgui::reactive::use_local_context::<Settings>()
        .and_then(|settings| settings.paths().cloned());
    let Some(paths) = paths else {
        workspace.complain("there is nowhere to keep sessions");
        return;
    };
    let root = workspace.project().root().to_path_buf();

    match leaf {
        "save" => {
            let order = workspace.order();
            let current = workspace.current_buffer().map(|buffer| buffer.id);
            let mut files = Vec::new();
            let mut showing = 0;

            for id in order {
                let Some(buffer) = workspace.buffer_untracked(id) else {
                    continue;
                };
                let Some(path) = buffer.path.clone() else {
                    continue;
                };
                if Some(id) == current {
                    showing = files.len();
                }
                // The caret of the window showing it, when one is; otherwise the top.
                let line = workspace
                    .handle_for(workspace.focused_untracked(), id)
                    .map_or(1, |handle| {
                        handle.query(|snapshot| {
                            let caret = snapshot.selections().primary().head;
                            snapshot.rope().byte_to_line(caret) as u64 + 1
                        })
                    });
                files.push(Entry {
                    path: workspace.project().relative(&path).into_owned().into(),
                    line,
                });
            }

            let saved = Session {
                root,
                files,
                showing,
            };
            match session::save(&paths, &saved) {
                Ok(path) => workspace.say(format!("session saved to {}", path.display())),
                Err(error) => workspace.complain(error.to_string()),
            }
        }
        "load" | "load_here" => match session::load(&paths, &root) {
            Ok(session) => restore(workspace, &session),
            Err(error) => workspace.complain(error.to_string()),
        },
        "load_last" => match session::most_recent(&paths) {
            Some(session) => restore(workspace, &session),
            None => workspace.say("no sessions"),
        },
        "delete" => match session::delete(&paths, &root) {
            Ok(()) => workspace.say("session deleted"),
            Err(error) => workspace.complain(error.to_string()),
        },
        other => workspace.say(format!("session.{other} is not built yet")),
    }
    let _ = handle;
}

/// Opens everything a session names, and shows what it was showing.
pub(super) fn restore(workspace: &Workspace, session: &crate::session::Session) {
    if session.files.is_empty() {
        workspace.say("that session has nothing in it");
        return;
    }
    for entry in &session.files {
        crate::files::open_at(workspace, session.absolute(entry), Some(entry.line));
    }
    // The one that was showing goes last, so it is the one left on screen. Every open before it
    // showed itself on the way past.
    if let Some(entry) = session.files.get(session.showing) {
        crate::files::open_at(workspace, session.absolute(entry), Some(entry.line));
    }
    workspace.say(format!("{} files", session.files.len()));
}
