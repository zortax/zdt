//! The pickers.

use super::*;
use crate::workspace::Workspace;
use zgui_editor::EditorHandle;

/// The pickers.
///
/// One action per source, all of them the same call: the difference between `<Leader>ff` and
/// `<Leader>fb` is which list is gathered, not what the modal does with it.
pub(super) fn run(
    workspace: &Workspace,
    leaf: &str,
    args: &zdt_vim::Args,
    handle: Option<&EditorHandle>,
) {
    use crate::picker::{Picker, Source};

    let Some(picker) = zgui::reactive::use_local_context::<Picker>() else {
        return;
    };

    // Four of these are questions for a language server. The picker could gather none of them
    // itself. They keep their `picker.*` names, because that is what the shipped keymap binds and
    // what anybody's fingers have learned. Each one is an LSP request whose answer happens to be
    // shown in a picker.
    match leaf {
        "references" | "symbols" | "workspace_symbols" | "diagnostics" => {
            let asked = match leaf {
                "references" => "references",
                "symbols" => "outline",
                "workspace_symbols" => "workspace_symbols",
                _ => "diagnostics",
            };
            lsp::run(workspace, asked, handle);
            return;
        }
        // Three more are questions for the repository, answered on a worker and shown the same
        // way. A picker and not the panel: somebody pressing `<Leader>gc` wants to *go* somewhere,
        // and a picker is what goes places.
        "git_status" | "git_commits" | "git_branches" => {
            git::picker(workspace, leaf);
            return;
        }
        _ => {}
    }

    let Some(mut source) = Source::named(leaf, args) else {
        workspace.say(format!("picker.{leaf} is not built yet"));
        return;
    };

    // `<Leader>fc` searches for what the caret is on, which is the one thing a picker cannot ask
    // for itself: by the time it is open, the caret is in its own prompt.
    if args.flag("word_under_cursor")
        && let Some(handle) = handle
    {
        let word = handle.query(|snapshot| {
            let caret = snapshot.selections().primary().head;
            let range = snapshot.word_at(caret);
            snapshot.text_in(range)
        });
        if let Source::Grep { start, .. } = &mut source {
            *start = word;
        }
    }

    picker.open(source);
}
