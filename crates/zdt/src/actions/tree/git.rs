//! What git can do to a row.
//!
//! The same six things the panel offers, reached from the tree so that a file can be staged where
//! it was just edited. Each one acts on everything picked out, or on the row the caret is on.
//!
//! Every one of them writes, and writing is blocking, so all of them go through a worker. What
//! comes back is one line saying what happened and a read of the marks.

use std::path::PathBuf;

use zdt_git::{Repo, ignore::Kind};

use crate::explorer::Explorer;
use crate::workspace::Workspace;

/// One row of the tree, as git names it.
struct Named {
    /// Its path from the top of the working tree.
    path: String,
    /// Whether it is a directory, which decides the shape of an ignore rule.
    directory: bool,
}

/// Carries `leaf` out on whatever the tree is acting on.
pub(super) fn run(workspace: &Workspace, explorer: &Explorer, leaf: &str) {
    let root = explorer.root();
    let Ok(repo) = Repo::open(&root) else {
        workspace.complain("this project is not in a git repository");
        return;
    };

    let paths = explorer.acting_on();
    let named: Vec<Named> = paths.iter().filter_map(|path| name(&repo, path)).collect();
    if named.is_empty() {
        workspace.complain("nothing to act on");
        return;
    }

    let word = done(leaf);
    let happened = done_past(leaf);
    let named_what = what(&paths);
    let ignoring = leaf == "ignore";
    let leaf = leaf.to_owned();

    let (workspace, explorer) = (workspace.clone(), explorer.clone());
    zdt_view::detached(async move {
        let outcome = zgui::task::blocking(move || {
            // What the index holds is files, and a row can be a directory. Whatever git names
            // under one is what a directory row stands for, which is what `git add <dir>` means.
            let changed = if wants_files(&leaf) {
                zdt_git::status::status(&repo).unwrap_or_default()
            } else {
                Vec::new()
            };
            let mut acted = 0_usize;
            for one in &named {
                for path in &spread(one, &leaf, &changed) {
                    act(&repo, &leaf, path, one.directory)?;
                    acted += 1;
                }
            }
            Ok::<usize, zdt_git::Error>(acted)
        })
        .await;

        match outcome {
            // A directory git says nothing about spreads to nothing. Saying so beats reporting
            // work that never happened.
            Ok(0) => workspace.say(format!("nothing to {word} in {named_what}")),
            Ok(_) => {
                // A new rule changes what the tree shows, and only a refresh reads the ignore
                // files again. Everything else changes a mark and leaves the rows where they are.
                if ignoring {
                    explorer.refresh();
                }
                crate::actions::files::touched();
                workspace.say(format!("{happened} {named_what}"));
            }
            Err(error) => workspace.complain(error.to_string()),
        }
    });
}

/// Whether `leaf` acts on files, so a directory row has to be spread over what is under it.
///
/// `ignore` writes a rule about the path itself, and `untrack` walks the index by prefix. Those
/// two take the row as it is.
fn wants_files(leaf: &str) -> bool {
    matches!(leaf, "stage" | "unstage" | "revert" | "discard")
}

/// What one row stands for: itself, or every changed path under it.
///
/// A directory git says nothing about spreads to nothing, and the row does nothing rather than
/// silently reporting that it did.
fn spread(one: &Named, leaf: &str, changed: &[zdt_git::Entry]) -> Vec<String> {
    if !one.directory || !wants_files(leaf) {
        return vec![one.path.clone()];
    }
    let under = format!("{}/", one.path);
    changed
        .iter()
        .filter(|entry| entry.path.starts_with(&under))
        .map(|entry| entry.path.clone())
        .collect()
}

/// Does one thing to one path.
fn act(repo: &Repo, leaf: &str, path: &str, directory: bool) -> Result<(), zdt_git::Error> {
    match leaf {
        "stage" => zdt_git::stage::stage_file(repo, path),
        "unstage" | "revert" => zdt_git::stage::unstage_file(repo, path),
        "discard" => zdt_git::stage::discard_file(repo, path),
        "untrack" => zdt_git::stage::untrack(repo, path),
        "ignore" => zdt_git::ignore::add(
            repo,
            path,
            if directory {
                Kind::Directory
            } else {
                Kind::File
            },
        ),
        _ => Ok(()),
    }
}

/// `path` as git names it, when it is inside the working tree.
fn name(repo: &Repo, path: &std::path::Path) -> Option<Named> {
    Some(Named {
        path: repo.relative(path)?,
        directory: path.is_dir(),
    })
}

/// The act, as a name: what there was "nothing to" do.
const fn done(leaf: &str) -> &'static str {
    match leaf.as_bytes() {
        b"stage" => "stage",
        b"unstage" | b"revert" => "unstage",
        b"discard" => "discard",
        b"untrack" => "untrack",
        _ => "ignore",
    }
}

/// The same, as what happened.
const fn done_past(leaf: &str) -> &'static str {
    match leaf.as_bytes() {
        b"stage" => "staged",
        b"unstage" => "unstaged",
        b"discard" => "discarded changes in",
        b"untrack" => "no longer tracking",
        b"ignore" => "ignoring",
        _ => "put back",
    }
}

/// What was acted on, as a person reads it: one name, or a count.
fn what(paths: &[PathBuf]) -> String {
    match paths {
        [one] => one
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        many => format!("{} files", many.len()),
    }
}
