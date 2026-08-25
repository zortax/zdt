//! The agent surface, wired into the editor.
//!
//! The surface itself is [`zdt_agentui`] and has never heard of a workspace. This is the other
//! half: what "open this project", "give me the keyboard" and "answer a key" mean inside this
//! editor, and the mounting that puts the sidebar and the chat into every window.

pub mod host;
pub mod resize;
pub mod view;

use zgui::reactive::RenderEffect;

use crate::session::host::SessionHost;

/// Builds the daemon connection and the surface state, and publishes both.
///
/// Called from the application's context, above every window, so the connection outlives them
/// all. The two effects started here are held by the application's scope on purpose.
pub fn install(sessions: &SessionHost, settings: &crate::settings::Settings) {
    let client = zdt_agent_client::AgentClient::install();
    let editor = std::rc::Rc::new(host::Editor::new(sessions.clone()));
    let agent = zdt_agentui::AgentUi::new(client.clone(), editor);

    // Whether the sidebar starts open: what the starting session last showed, and the
    // configuration for a session that never wrote it down.
    let side_open = sessions
        .first_session()
        .and_then(|session| session.agent_view().side_open)
        .unwrap_or_else(|| settings.with_untracked(|config| config.agent.open));
    agent.set_open(side_open);

    // A thread the daemon just made is the one to look at: select it, follow it, and show the
    // chat. The shells and the answer race, so this waits for the row to arrive.
    let opening = {
        let (client, agent) = (client.clone(), agent.clone());
        RenderEffect::new(move |_| {
            let Some(created) = client.created() else {
                return;
            };
            let threads = client.threads();
            if !threads.iter().any(|shell| shell.id == created) {
                return;
            }
            let _ = client.take_created();
            // The new thread must land somewhere visible: the sidebar shows it, and the screen
            // stays whichever face it was.
            agent.set_open(true);
            agent.open_thread(created);
        })
    };
    std::mem::forget(opening);

    // The selection follows the session on screen, and every arrival of the daemon's rows says
    // the answer again: the rows land after the first window is drawn, and a directory whose
    // threads had not arrived yet would otherwise keep the answer it was given before them.
    let settling = {
        let (client, agent) = (client.clone(), agent.clone());
        RenderEffect::new(move |_| {
            let _ = agent.here();
            let _ = client.threads();
            let _ = client.has_listed();
            agent.settle_selection();
        })
    };
    std::mem::forget(settling);

    restore_face(sessions, &client, &agent);

    zdt_agentui::provide(agent);
}

/// Puts back the face the starting session last showed, once.
///
/// Only at startup: switching sessions later keeps whichever face is on screen, so the saved
/// state is never applied to a running window.
fn restore_face(
    sessions: &SessionHost,
    client: &zdt_agent_client::AgentClient,
    agent: &zdt_agentui::AgentUi,
) {
    let Some(session) = sessions.first_session() else {
        return;
    };
    let view = session.agent_view();
    if view.face != crate::session::schema::FaceSort::Agent {
        return;
    }
    agent.set_open(true);
    agent.to_chat();
    // Said to the session directly: `to_chat` reaches the focus through the calling subtree's
    // context, and there is no subtree yet.
    session.workspace().focus().enter_agent();

    // The chat's thread comes back once the daemon's rows arrive. A thread rooted somewhere
    // else is left alone: selecting it would switch the session before anybody asked.
    let Some(wanted) = view.thread.map(zdt_agent::thread::ThreadId) else {
        return;
    };
    let root = session.project().root().to_path_buf();
    let (client, agent) = (client.clone(), agent.clone());
    let landing = RenderEffect::new(move |done: Option<bool>| {
        if done == Some(true) {
            return true;
        }
        let threads = client.threads();
        if threads.is_empty() {
            return false;
        }
        let Some(index) = threads.iter().position(|shell| shell.id == wanted) else {
            // The daemon answered and the thread is gone. The face stays; the chat is empty.
            return true;
        };
        if threads[index].root == root {
            agent.select(&threads[index]);
        }
        true
    });
    std::mem::forget(landing);
}
