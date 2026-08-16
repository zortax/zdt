//! The command line.

use crate::workspace::Workspace;

/// The command line.
///
/// `:` opens an empty one. From a visual selection it opens holding `'<,'>`, so the range is
/// already there. Vim does the same, and it makes `:'<,'>s/a/b/` two keys.
pub(super) fn run(workspace: &Workspace, leaf: &str, args: &zdt_vim::Args) {
    let Some(cmdline) = zgui::reactive::use_local_context::<crate::cmdline::CommandLine>() else {
        return;
    };
    match leaf {
        "open" => cmdline.open(args.str("start").unwrap_or("")),
        other => workspace.say(format!("cmdline.{other} is not built yet")),
    }
}
