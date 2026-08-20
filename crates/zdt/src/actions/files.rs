//! Making, renaming, deleting and pasting files.

use crate::explorer::Explorer;
use crate::explorer::field::{About, At, Field};
use crate::settings::Settings;
use crate::workspace::Workspace;
use std::path::PathBuf;

/// Where a field opens: where the pointer opened the menu, or on the caret row.
fn opening_at(explorer: &Explorer) -> At {
    match crate::explorer::menu::taken_at() {
        Some((x, y)) => At::Pointer(x, y),
        None => At::Row(explorer.at_untracked()),
    }
}

/// Says the working tree has just been written to, so the tree's marks are read again.
///
/// Looked up rather than passed: every caller here is already inside a task, and what it needs is
/// one nudge on the way out.
pub(super) fn touched() {
    if let Some(status) = zgui::reactive::use_local_context::<crate::git::Status>() {
        status.refresh_soon();
    }
}

/// Changes a setting and answers what it became, when there are settings to change.
pub(super) fn with_settings<T>(change: impl FnOnce(&mut zdt_core::Config) -> T) -> Option<T> {
    let settings = zgui::reactive::use_local_context::<Settings>()?;
    let mut answer = None;
    settings.update(|config| answer = Some(change(config)));
    answer
}

/// Asks for a name, then makes one.
pub(super) fn create(workspace: &Workspace, explorer: &Explorer, directory: bool) {
    let target = explorer.target_directory();
    let about = if directory {
        About::NewDirectory
    } else {
        About::NewFile
    };
    ask(workspace, explorer, about, String::new(), move |name| {
        let path = target.join(name.trim_end_matches('/'));
        // A name ending in a separator is a directory, which is how neo-tree's `a` makes one.
        let directory = directory || name.ends_with('/');
        let made = path.clone();
        (
            Box::new(move || zdt_core::paths::create(&path, directory).map(|_| ()))
                as Box<dyn FnOnce() -> std::io::Result<()> + Send>,
            Some(made),
        )
    });
}

/// Asks for a new name, then moves it.
pub(super) fn rename(workspace: &Workspace, explorer: &Explorer) {
    let Some(row) = explorer.selected() else {
        return;
    };
    let from = row.entry.path.clone();
    let parent = from
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();
    ask(
        workspace,
        explorer,
        About::Rename,
        row.entry.name.clone(),
        move |name| {
            // Cloned per call: the field's answer is typed as callable more than once, even
            // though it only ever is once.
            let from = from.clone();
            let to = parent.join(name);
            let landed = to.clone();
            (
                Box::new(move || zdt_core::paths::rename(&from, &to))
                    as Box<dyn FnOnce() -> std::io::Result<()> + Send>,
                Some(landed),
            )
        },
    );
}

/// Asks where it should go, then moves it.
///
/// The field opens holding the path from the top of the project, so moving something two
/// directories along is one edit rather than a name typed out again.
pub(super) fn move_to(workspace: &Workspace, explorer: &Explorer) {
    let Some(row) = explorer.selected() else {
        return;
    };
    let from = row.entry.path.clone();
    let root = explorer.root();
    let start = workspace.project().relative(&from).into_owned();

    ask(
        workspace,
        explorer,
        crate::explorer::field::About::Move,
        start,
        move |wanted| {
            // Cloned per call: the field's answer is typed as callable more than once, even though
            // it only ever is once.
            let from = from.clone();
            let to = root.join(wanted.trim_start_matches('/'));
            let landed = to.clone();
            (
                Box::new(move || {
                    if let Some(parent) = to.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    zdt_core::paths::rename(&from, &to)
                }) as Box<dyn FnOnce() -> std::io::Result<()> + Send>,
                Some(landed),
            )
        },
    );
}

/// Asks whether, then removes it.
pub(super) fn delete(workspace: &Workspace, explorer: &Explorer) {
    let Some(row) = explorer.selected() else {
        return;
    };
    let path = row.entry.path.clone();
    // A typed confirmation, and no dialog. The keyboard is already in the tree, and a dialog that
    // takes it away is one people dismiss without reading.
    ask(
        workspace,
        explorer,
        About::Delete,
        String::new(),
        move |answer| {
            let path = path.clone();
            if !answer.eq_ignore_ascii_case("y") && !answer.eq_ignore_ascii_case("yes") {
                return (
                    Box::new(|| Ok(())) as Box<dyn FnOnce() -> std::io::Result<()> + Send>,
                    None,
                );
            }
            (
                Box::new(move || zdt_core::paths::remove(&path))
                    as Box<dyn FnOnce() -> std::io::Result<()> + Send>,
                None,
            )
        },
    );
}

/// Moves `from` into `into`, which is what a drop in the tree does.
///
/// Public because the tree's pointer handling calls it: a drag is not a key, so it has no action
/// name, but what it does is the same work `p` does after an `x`.
pub fn move_into(
    workspace: &Workspace,
    explorer: &Explorer,
    from: &std::path::Path,
    into: &std::path::Path,
) {
    let Some(name) = from.file_name() else {
        return;
    };
    let target = into.join(name);
    let (from, explorer, workspace) = (from.to_path_buf(), explorer.clone(), workspace.clone());

    zdt_view::detached(async move {
        let done = zgui::task::blocking(move || {
            let to = zdt_core::paths::free_name(&target);
            zdt_core::paths::rename(&from, &to).map(|()| to)
        })
        .await;
        match done {
            Ok(landed) => {
                explorer.refresh();
                touched();
                explorer.reveal(&landed);
                workspace.say(format!("moved to {}", landed.display()));
            }
            Err(error) => workspace.complain(error.to_string()),
        }
    });
}

/// Puts what was held into the selected directory.
pub(super) fn paste(workspace: &Workspace, explorer: &Explorer) {
    let Some(held) = explorer.clipboard() else {
        workspace.complain("nothing to paste");
        return;
    };
    let target = explorer.target_directory().join(
        held.path
            .file_name()
            .map(std::ffi::OsStr::to_os_string)
            .unwrap_or_default(),
    );
    if target == held.path {
        // Pasting into the directory it already sits in means a copy beside it, not an error.
        if held.cut {
            explorer.release();
            return;
        }
    }

    let from = held.path.clone();
    let cut = held.cut;
    let explorer = explorer.clone();
    let workspace = workspace.clone();
    zdt_view::detached(async move {
        let done = zgui::task::blocking(move || {
            let to = zdt_core::paths::free_name(&target);
            if cut {
                zdt_core::paths::rename(&from, &to).map(|()| to)
            } else {
                zdt_core::paths::copy(&from, &to).map(|()| to)
            }
        })
        .await;
        match done {
            Ok(landed) => {
                if cut {
                    explorer.release();
                }
                explorer.refresh();
                touched();
                workspace.say(format!(
                    "{} {}",
                    if cut { "moved to" } else { "copied to" },
                    landed.display()
                ));
            }
            Err(error) => workspace.complain(error.to_string()),
        }
    });
}

/// The shape every tree question has: ask, do the work on a worker, refresh, report.
///
/// `plan` turns the answer into the work and, when there is one, the path the caret should land
/// on afterwards. It runs on the interface thread; only what it returns crosses to the worker.
pub(super) fn ask<F>(
    workspace: &Workspace,
    explorer: &Explorer,
    about: About,
    start: String,
    plan: F,
) where
    F: Fn(
            &str,
        ) -> (
            Box<dyn FnOnce() -> std::io::Result<()> + Send>,
            Option<PathBuf>,
        ) + 'static,
{
    let Some(field) = zgui::reactive::use_local_context::<Field>() else {
        return;
    };
    let at = opening_at(explorer);
    let explorer = explorer.clone();
    let workspace = workspace.clone();
    field.ask(about, start, at, move |answer| {
        let (work, landing) = plan(answer);
        let explorer = explorer.clone();
        let workspace = workspace.clone();
        // Detached, because submitting the field is what closed it: a task belonging to the field
        // would be cancelled before the file was ever made.
        zdt_view::detached(async move {
            match zgui::task::blocking(work).await {
                Ok(()) => {
                    explorer.refresh();
                    touched();
                    if let Some(landing) = landing {
                        explorer.reveal(&landing);
                    }
                }
                Err(error) => workspace.complain(error.to_string()),
            }
            explorer.focus();
        });
    });
}
