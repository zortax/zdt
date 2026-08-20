//! Moving between sessions.
//!
//! Every leaf here goes through [`crate::session::host`]. There is nothing to save and nothing to
//! load: a session writes itself down as it is worked in and comes back when its directory is
//! opened again, so the only things left to ask for are which session and where.

use crate::workspace::Workspace;
use zgui_editor::EditorHandle;

/// The sessions.
pub(super) fn run(workspace: &Workspace, leaf: &str, handle: Option<&EditorHandle>) {
    let _ = handle;
    match leaf {
        "pick" | "sessionize" => pick(workspace, crate::session::pick::Where::Here),
        "pick_window" => pick(workspace, crate::session::pick::Where::NewWindow),
        "next" => step(workspace, 1),
        "prev" => step(workspace, -1),
        "new" => new(workspace),
        "kill" => kill(workspace),
        "forget" => forget(workspace),
        "detach" => detach(workspace),
        other => workspace.say(format!("session.{other} is not built yet")),
    }
}

/// `<Leader>Sf`: the sessionizer, showing what is chosen where `place` says.
fn pick(workspace: &Workspace, place: crate::session::pick::Where) {
    let Some(session) = crate::session::use_session() else {
        workspace.complain("there is no session here");
        return;
    };
    crate::session::pick::open(&session, place);
}

/// Walks to the session `by` places along, wrapping.
///
/// The order is the registry's, which is the order they were opened in. That is the same order
/// `]b` walks the buffer line in, and for the same reason: a list somebody built by opening
/// things is a list they can predict.
fn step(workspace: &Workspace, by: isize) {
    let Some(host) = zgui::reactive::use_local_context::<crate::session::host::SessionHost>()
    else {
        return;
    };
    let Some(session) = crate::session::use_session() else {
        return;
    };
    let open = host.list_untracked();
    if open.len() < 2 {
        workspace.say("this is the only session");
        return;
    }
    let Some(at) = open.iter().position(|listed| listed.id == session.id()) else {
        return;
    };
    let count = open.len() as isize;
    let next = (at as isize + by).rem_euclid(count) as usize;
    host.reveal(open[next].key.clone(), &[]);
}

/// `<Leader>Sn`: a session on a directory somebody types.
fn new(workspace: &Workspace) {
    let Some(prompt) = zgui::reactive::use_local_context::<crate::prompt::Prompt>() else {
        return;
    };
    let Some(host) = zgui::reactive::use_local_context::<crate::session::host::SessionHost>()
    else {
        return;
    };
    let workspace = workspace.clone();
    prompt.ask("Session in", "", move |typed| {
        let path = zdt_core::config::expand_home(typed);
        match crate::session::SessionKey::of(&path) {
            Some(key) => {
                host.reveal(key, &[]);
            }
            None => workspace.complain(format!("{} is not a directory", path.display())),
        }
    });
}

/// `<Leader>Sk`: takes this session away, stopping its servers and its programs.
fn kill(workspace: &Workspace) {
    let Some(host) = zgui::reactive::use_local_context::<crate::session::host::SessionHost>()
    else {
        return;
    };
    let Some(session) = crate::session::use_session() else {
        return;
    };
    // Somewhere to go first: killing what is on screen with nothing to replace it would leave a
    // window drawing nothing.
    let open = host.list_untracked();
    let Some(other) = open.iter().find(|listed| listed.id != session.id()) else {
        workspace.say("this is the only session");
        return;
    };
    let name = session.name();
    let key = other.key.clone();
    let id = session.id();
    host.reveal(key, &[]);
    if host.kill(id) {
        workspace.say(format!("killed {name}"));
    }
}

/// `<Leader>SD`: closes this window and leaves its sessions running.
///
/// tmux's word. The sessions keep their servers, their programs and their buffers, and the next
/// window to show one finds all three.
fn detach(workspace: &Workspace) {
    let Some(host) = zgui::reactive::use_local_context::<crate::session::host::SessionHost>()
    else {
        return;
    };
    let Some(client) = zgui::reactive::use_local_context::<crate::session::client::Client>() else {
        return;
    };

    // The last window cannot be detached from: closing it stops the application, which is
    // quitting and not detaching, and there is a key for that already.
    if host.clients().len() < 2 {
        workspace.say("this is the only window; <Leader>qq quits");
        return;
    }
    if let Some(handle) = client.handle() {
        handle.close();
    }
}

/// `<Leader>Sd`: forgets what this session wrote down, leaving it running.
///
/// The live session is untouched: what goes is the copy on disk, so opening this directory again
/// starts from nothing rather than from where somebody left it. Distinct from `session.kill`,
/// which stops the session but leaves what it wrote for the next time.
fn forget(workspace: &Workspace) {
    let Some(session) = crate::session::use_session() else {
        return;
    };
    let Some(state) = session.state() else {
        workspace.complain("there is nowhere sessions are kept");
        return;
    };
    match crate::session::store::delete(&state, session.project().root()) {
        Ok(()) => workspace.say(format!("forgot what {} had saved", session.name())),
        Err(error) => workspace.complain(error.to_string()),
    }
}
