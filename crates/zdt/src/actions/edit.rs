//! Applying what a server asks the editor to change.
//!
//! A `WorkspaceEdit` is the answer to a rename, to most code actions, and to "organize imports".
//! All three need exactly the same thing done with it, and all three corrupt files silently when
//! it is done wrong — which is why it is written once, here, rather than three times.
//!
//! # The four rules
//!
//! **Back to front.** Every range in an edit is against the text as it was, and applying one moves
//! everything after it. Sorted descending by start, each edit lands where the server said.
//!
//! **One transaction per file.** All of a file's edits go in a single `ReplaceRanges`, so undo
//! takes back the whole rename rather than one occurrence of it.
//!
//! **Versioned edits are checked.** A server that names a version is a server saying "this edit is
//! against exactly that text". If the buffer has moved on since, the edit is refused rather than
//! applied to text it was not computed for. This is the same rule the diagnostics already follow,
//! for the same reason: applying a stale answer is worse than applying none.
//!
//! **Files it names are opened.** A rename crosses files, and most of them are not open. Each is
//! opened, edited, and left open — because somebody who renamed a symbol in nine files wants to be
//! able to look at what happened in all nine, and to undo it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use lsp_types::{DocumentChangeOperation, DocumentChanges, OneOf, ResourceOp, TextEdit, Url};

use crate::workspace::Workspace;

/// What applying an edit did.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Applied {
    /// How many files were changed.
    pub files: usize,
    /// How many individual edits went in.
    pub edits: usize,
    /// How many files were created, renamed or removed.
    pub operations: usize,
    /// How many files were named that could not be changed.
    ///
    /// A file whose buffer has moved past the version the edit was computed against, or one whose
    /// URI is not a file at all.
    pub refused: usize,
}

impl Applied {
    /// What to tell somebody, in the words they would use.
    #[must_use]
    pub fn summary(&self) -> String {
        if self.files == 0 && self.operations == 0 {
            return "nothing to change".to_owned();
        }
        let files = match self.files {
            1 => "1 file".to_owned(),
            many => format!("{many} files"),
        };
        let mut said = format!("changed {files}");
        if self.operations > 0 {
            said.push_str(&format!(", {} moved", self.operations));
        }
        if self.refused > 0 {
            said.push_str(&format!(", {} refused", self.refused));
        }
        said
    }
}

/// Applies `edit`, opening whatever files it names.
///
/// Everything that can be done now is done now; what needs a file opened is done once the file has
/// been read, which is a frame or two later. The count reported is of what was asked for rather
/// than of what has finished, because the alternative is a message that arrives after the person
/// has moved on.
pub fn apply(
    workspace: &Workspace,
    notify: Option<&crate::notify::Notify>,
    edit: lsp_types::WorkspaceEdit,
    encoding: zdt_lsp::Encoding,
) {
    let mut applied = Applied::default();

    // The resource operations first, in the order the server gave them: a rename that moves a file
    // and then edits it has to move it before the edit names the new path.
    if let Some(DocumentChanges::Operations(operations)) = edit.document_changes.as_ref() {
        for operation in operations {
            if let DocumentChangeOperation::Op(op) = operation {
                if resource(op) {
                    applied.operations += 1;
                } else {
                    applied.refused += 1;
                }
            }
        }
    }

    for (path, edits, version) in grouped(edit) {
        if edits.is_empty() {
            continue;
        }
        applied.files += 1;
        applied.edits += edits.len();
        into_file(workspace, path, edits, version, encoding);
    }

    // Handed in rather than looked up: this is called from inside a task, and a context looked up
    // after an await is not there — see `tests/context.rs`.
    match notify {
        Some(notify) => notify.say(applied.summary()),
        None => workspace.say(applied.summary()),
    }
}

/// Every file an edit names, with what to do to it and which version it was computed against.
///
/// The two shapes the protocol allows — a plain map, and a list of versioned documents — flattened
/// into one, so that everything below has a single thing to deal with.
fn grouped(edit: lsp_types::WorkspaceEdit) -> Vec<(PathBuf, Vec<TextEdit>, Option<i32>)> {
    let mut out: BTreeMap<PathBuf, (Vec<TextEdit>, Option<i32>)> = BTreeMap::new();

    let mut put = |uri: &Url, edits: Vec<TextEdit>, version: Option<i32>| {
        let Some(path) = zdt_lsp::convert::path_of(uri) else {
            return;
        };
        let entry = out.entry(path).or_insert_with(|| (Vec::new(), version));
        entry.0.extend(edits);
        // The lower version wins: if a server names two versions for one file, the edit is only
        // safe against the older of them.
        entry.1 = match (entry.1, version) {
            (Some(held), Some(now)) => Some(held.min(now)),
            (held, now) => held.or(now),
        };
    };

    match edit.document_changes {
        Some(DocumentChanges::Edits(documents)) => {
            for document in documents {
                let version = document.text_document.version;
                let edits = document
                    .edits
                    .into_iter()
                    .map(|one| match one {
                        OneOf::Left(edit) => edit,
                        // An annotated edit is an ordinary one with a note about why, and the note
                        // is for a interface that offers to apply edits selectively. This one
                        // applies all of them.
                        OneOf::Right(annotated) => annotated.text_edit,
                    })
                    .collect();
                put(&document.text_document.uri, edits, version);
            }
        }
        Some(DocumentChanges::Operations(operations)) => {
            for operation in operations {
                if let DocumentChangeOperation::Edit(document) = operation {
                    let version = document.text_document.version;
                    let edits = document
                        .edits
                        .into_iter()
                        .map(|one| match one {
                            OneOf::Left(edit) => edit,
                            OneOf::Right(annotated) => annotated.text_edit,
                        })
                        .collect();
                    put(&document.text_document.uri, edits, version);
                }
            }
        }
        None => {}
    }

    // The old shape, which servers still send: a map with no versions in it at all.
    if let Some(changes) = edit.changes {
        for (uri, edits) in changes {
            put(&uri, edits, None);
        }
    }

    out.into_iter()
        .map(|(path, (edits, version))| (path, edits, version))
        .collect()
}

/// Puts `edits` into the file at `path`, opening it if it is not open.
fn into_file(
    workspace: &Workspace,
    path: PathBuf,
    edits: Vec<TextEdit>,
    version: Option<i32>,
    encoding: zdt_lsp::Encoding,
) {
    if let Some(buffer) = workspace.find_path(&path) {
        let window = workspace.focused_untracked();
        if let Some(handle) = workspace.handle_for(window, buffer) {
            write(&handle, &edits, encoding);
            return;
        }
    }

    // Not open, or open with no editor mounted on it. Open it and edit it once it is there: the
    // buffer is read on a worker and the editor is built on the frame after that, so this cannot
    // be done in one go.
    crate::files::open(workspace, path.clone());

    let Some(timers) = zgui::view::time::Timers::current() else {
        return;
    };
    let workspace = workspace.clone();
    let handle = timers.set_timeout(std::time::Duration::from_millis(60), move || {
        let Some(buffer) = workspace.find_path(&path) else {
            return;
        };
        let window = workspace.focused_untracked();
        let Some(editor) = workspace.handle_for(window, buffer) else {
            return;
        };
        // The version is not checked here: a file that has just been read from disk is at the
        // version on disk, which is the one the server computed against.
        let _ = version;
        write(&editor, &edits, encoding);
    });
    // Held nowhere: dropping a timer handle cancels it, and this one has to fire.
    std::mem::forget(handle);
}

/// Puts `edits` into `handle`, as one transaction.
fn write(handle: &zgui_editor::EditorHandle, edits: &[TextEdit], encoding: zdt_lsp::Encoding) {
    let mut replacements: Vec<(std::ops::Range<usize>, String)> = handle.query(|snapshot| {
        edits
            .iter()
            .map(|edit| {
                (
                    zdt_lsp::convert::range_of(snapshot.rope(), edit.range, encoding),
                    edit.new_text.clone(),
                )
            })
            .collect()
    });
    // Back to front, because every range is against the text as it was and applying one moves
    // everything after it.
    replacements.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    handle.command(zgui_editor::Command::ReplaceRanges(replacements));
}

/// Creates, renames or removes a file, as a server asked.
///
/// Answers whether it worked. The operations themselves are [`zdt_core::paths`]'s, which are the
/// same ones the file tree uses — so a rename from a language server and a rename from `r` in the
/// tree do exactly the same thing.
fn resource(op: &ResourceOp) -> bool {
    match op {
        ResourceOp::Create(create) => {
            let Some(path) = zdt_lsp::convert::path_of(&create.uri) else {
                return false;
            };
            let overwrite = create
                .options
                .as_ref()
                .and_then(|options| options.overwrite)
                .unwrap_or(false);
            if path.exists() && !overwrite {
                // `ignore_if_exists` and a plain refusal come to the same thing here: the file is
                // there and the server did not ask to replace it.
                return true;
            }
            zdt_core::paths::create(&path, false).is_ok()
        }
        ResourceOp::Rename(rename) => {
            let (Some(from), Some(to)) = (
                zdt_lsp::convert::path_of(&rename.old_uri),
                zdt_lsp::convert::path_of(&rename.new_uri),
            ) else {
                return false;
            };
            zdt_core::paths::rename(&from, &to).is_ok()
        }
        ResourceOp::Delete(delete) => {
            let Some(path) = zdt_lsp::convert::path_of(&delete.uri) else {
                return false;
            };
            zdt_core::paths::remove(&path).is_ok()
        }
    }
}

#[cfg(test)]
mod tests {
    use lsp_types::{
        DocumentChanges, OneOf, OptionalVersionedTextDocumentIdentifier, Position, Range,
        TextDocumentEdit, TextEdit, Url, WorkspaceEdit,
    };

    use super::{Applied, grouped};

    fn uri(path: &str) -> Url {
        Url::parse(&format!("file://{path}")).expect("a url")
    }

    fn edit(line: u32, text: &str) -> TextEdit {
        TextEdit {
            range: Range::new(Position::new(line, 0), Position::new(line, 3)),
            new_text: text.to_owned(),
        }
    }

    #[test]
    fn the_old_shape_of_edit_still_reads() {
        // Servers still send a plain map with no versions in it, and a rename that silently did
        // nothing for those servers would be a rename that silently did nothing.
        let mut changes = std::collections::HashMap::new();
        changes.insert(uri("/project/a.rs"), vec![edit(0, "new")]);
        let found = grouped(WorkspaceEdit {
            changes: Some(changes),
            ..WorkspaceEdit::default()
        });

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, std::path::Path::new("/project/a.rs"));
        assert_eq!(found[0].1.len(), 1);
        assert_eq!(found[0].2, None, "the old shape carries no version");
    }

    #[test]
    fn the_versioned_shape_keeps_its_version() {
        let found = grouped(WorkspaceEdit {
            document_changes: Some(DocumentChanges::Edits(vec![TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier {
                    uri: uri("/project/a.rs"),
                    version: Some(7),
                },
                edits: vec![OneOf::Left(edit(0, "new"))],
            }])),
            ..WorkspaceEdit::default()
        });

        assert_eq!(found[0].2, Some(7));
    }

    #[test]
    fn one_file_named_twice_is_one_file() {
        // Which a server does when a rename touches a declaration and a use in the same file, and
        // two `ReplaceRanges` would be two undo steps for one rename.
        let found = grouped(WorkspaceEdit {
            document_changes: Some(DocumentChanges::Edits(vec![
                TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier {
                        uri: uri("/project/a.rs"),
                        version: Some(7),
                    },
                    edits: vec![OneOf::Left(edit(0, "new"))],
                },
                TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier {
                        uri: uri("/project/a.rs"),
                        version: Some(4),
                    },
                    edits: vec![OneOf::Left(edit(9, "new"))],
                },
            ])),
            ..WorkspaceEdit::default()
        });

        assert_eq!(found.len(), 1, "one file, one transaction");
        assert_eq!(found[0].1.len(), 2);
        assert_eq!(
            found[0].2,
            Some(4),
            "the older version, because that is the one the whole edit is safe against"
        );
    }

    #[test]
    fn a_uri_that_is_not_a_file_is_left_out_rather_than_crashing() {
        let mut changes = std::collections::HashMap::new();
        changes.insert(
            Url::parse("untitled:Untitled-1").expect("a url"),
            vec![edit(0, "new")],
        );
        assert!(
            grouped(WorkspaceEdit {
                changes: Some(changes),
                ..WorkspaceEdit::default()
            })
            .is_empty()
        );
    }

    #[test]
    fn nothing_at_all_is_nothing_at_all() {
        assert!(grouped(WorkspaceEdit::default()).is_empty());
    }

    #[test]
    fn what_is_said_afterwards_reads_like_something_a_person_would_say() {
        assert_eq!(Applied::default().summary(), "nothing to change");
        assert_eq!(
            Applied {
                files: 1,
                edits: 3,
                ..Applied::default()
            }
            .summary(),
            "changed 1 file"
        );
        assert_eq!(
            Applied {
                files: 9,
                edits: 20,
                operations: 1,
                refused: 2,
            }
            .summary(),
            "changed 9 files, 1 moved, 2 refused"
        );
    }
}
