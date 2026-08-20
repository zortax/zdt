//! What every named action does.
//!
//! The engine knows `motion.word_forward` and `operator.delete`. Everything else reaches here as
//! a name and some arguments, straight out of the keymap file: the pickers, the buffers, the
//! windows, the language servers. One `match` is the whole registry.
//!
//! An action nobody has written yet says so in the status line. That is what makes a half-built
//! editor say which half.

mod app;
mod buffer;
mod cmdline;
mod edit;
mod files;
mod git;
mod leap;
pub mod lsp;
mod picker;
mod popups;
mod session;
mod terminal;
mod tree;
mod window;

// The file tree's drag-and-drop reaches for this. A drag is not a key, so it has no action name.
pub use crate::actions::files::move_into;

use zdt_vim::Action;
use zgui_editor::EditorHandle;

use crate::vim::Vim;
use crate::workspace::Workspace;

/// Carries out `action`.
pub fn run(workspace: &Workspace, vim: &Vim, action: &Action, handle: Option<&EditorHandle>) {
    let leaf = action.leaf();
    let args = &action.args;

    match action.name.split('.').next().unwrap_or("") {
        "buffer" => self::buffer::run(workspace, leaf, args),
        "window" => self::window::run(workspace, vim, leaf, args),
        "app" => self::app::run(workspace, leaf),
        "editor" => self::leap::editor(handle, leaf),
        "leap" => self::leap::run(workspace, vim, leaf, handle),
        "tree" => self::tree::run(workspace, vim, leaf, args),
        "picker" => self::picker::run(workspace, leaf, args, handle),
        "terminal" => self::terminal::run(workspace, vim, leaf, args),
        "lsp" => lsp::run(workspace, leaf, handle),
        "git" => git::run(workspace, leaf, handle),
        "session" => self::session::run(workspace, leaf, handle),
        "cmdline" => self::cmdline::run(workspace, leaf, args),
        "diagnostic" => lsp::diagnostic(workspace, leaf, handle),
        "hover" => self::popups::hover(leaf),
        "completion" => self::popups::completion(workspace, leaf, handle),
        "gitpanel" => zdt_gitui::actions::run(leaf),
        "ui" => self::app::toggle(workspace, leaf, args),
        // Everything else belongs to a part of the editor that is still being built. Saying so is
        // better than a key that quietly does nothing.
        _ => workspace.say(format!("{} is not built yet", action.name)),
    }
}
