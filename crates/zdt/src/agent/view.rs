//! Mounting the surface into one session's shell.

use zdt_agentui::chat::AgentViewProps;
use zdt_agentui::chat::{CommitModalProps, WorkflowModalProps};
use zdt_agentui::sidebar::AgentSidebarProps;
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RenderEffect};
use zgui::{component, view};

use crate::agent::resize::AgentResizeProps;
use crate::app::frame::WindowControlsProps;
use crate::focus::{Sink, Spot};

/// The surface's pieces, inside one session's shell.
///
/// One per mounted session, like everything else in a shell: each session's focus map gets its
/// own agent sink, and the projector answers only for the session on screen.
#[component]
pub fn AgentMount(
    /// Whether this shell is the one on screen.
    showing: Signal<bool, LocalStorage>,
) -> impl IntoView {
    let agent = zdt_agentui::use_agent();
    let focus = crate::focus::use_focus();

    // Where the keyboard lands: the list's own box, the timeline, the review, or the composer's
    // editor.
    let sidebar = NodeRef::new();
    let chat = NodeRef::new();
    let composer = NodeRef::new();
    let review = NodeRef::new();
    let commit = NodeRef::new();
    let workflow = NodeRef::new();

    let sinking = {
        let (agent, focus) = (agent.clone(), focus.clone());
        RenderEffect::new(move |_| {
            let sink = match agent.wants() {
                zdt_agentui::Want::Composer => Sink::Node(composer),
                zdt_agentui::Want::Chat => Sink::Node(chat),
                zdt_agentui::Want::List => Sink::Node(sidebar),
                zdt_agentui::Want::Review => Sink::Node(review),
                zdt_agentui::Want::Commit => Sink::Node(commit),
                zdt_agentui::Want::Workflow => Sink::Node(workflow),
                zdt_agentui::Want::Filter => Sink::Node(agent.search_node().unwrap_or(sidebar)),
            };
            focus.register(Spot::Agent, sink);
        })
    };
    on_cleanup_local(move || drop(sinking));

    // What this shell shows of the agent surface, written down for the next start. Only the
    // shell on screen records; a hidden session keeps what it last showed.
    let remembering = {
        let agent = agent.clone();
        let session = crate::session::use_session();
        RenderEffect::new(move |_| {
            if !showing.get() {
                return;
            }
            let Some(session) = session.as_ref() else {
                return;
            };
            let face = match agent.screen() {
                zdt_agentui::Screen::Agent => crate::session::schema::FaceSort::Agent,
                zdt_agentui::Screen::Editor => crate::session::schema::FaceSort::Editor,
            };
            session.set_agent_view(crate::session::schema::AgentSnapshot {
                face,
                thread: agent.selected().map(|id| id.0),
                side_open: Some(agent.is_open()),
            });
        })
    };
    on_cleanup_local(move || drop(remembering));

    // What the daemon last refused or broke, announced from the shell on screen and no other.
    let complaining = {
        let agent = agent.clone();
        RenderEffect::new(move |_| {
            if !showing.get() {
                return;
            }
            if agent.client().problem().is_some()
                && let Some(problem) = agent.client().take_problem()
            {
                crate::notify::fail("agent", Some(problem));
            }
        })
    };
    on_cleanup_local(move || drop(complaining));

    // News about threads a person is not looking at, told from the shell on screen and no other.
    // A finished turn also re-reads what its thread changed on disk, when the thread works in
    // this shell's directory.
    let telling = {
        let agent = agent.clone();
        let session = crate::session::use_session();
        RenderEffect::new(move |_| {
            if !showing.get() {
                return;
            }
            if !agent.client().has_news() {
                return;
            }
            let watched = (agent.screen() == zdt_agentui::Screen::Agent)
                .then(|| agent.selected_untracked())
                .flatten();
            for notice in agent.client().take_news() {
                if let zdt_agent_client::Notice::Done { thread, .. } = &notice
                    && let Some(session) = session.as_ref()
                {
                    let root = agent
                        .client()
                        .threads_untracked()
                        .into_iter()
                        .find(|shell| shell.id == *thread)
                        .map(|shell| shell.root);
                    if root.as_deref() == Some(session.project().root()) {
                        crate::files::refresh_from_disk(session.workspace());
                    }
                }
                if Some(notice.thread()) == watched {
                    continue;
                }
                match notice {
                    zdt_agent_client::Notice::Done { title, .. } => {
                        crate::notify::say(format!("{title}: the turn finished"));
                    }
                    zdt_agent_client::Notice::Failed { title, error, .. } => {
                        let said = if error.is_empty() {
                            format!("{title}: the turn failed")
                        } else {
                            format!("{title}: {error}")
                        };
                        crate::notify::fail("agent", Some(said));
                    }
                    zdt_agent_client::Notice::Asking { title, .. } => {
                        crate::notify::warn(format!("{title} is waiting on you"));
                    }
                }
            }
        })
    };
    on_cleanup_local(move || drop(telling));

    // What a git action just did, said from the shell on screen. A revert or a commit moves the
    // disk, so the session re-reads what changed.
    let noting = {
        let agent = agent.clone();
        let session = crate::session::use_session();
        RenderEffect::new(move |_| {
            if !showing.get() {
                return;
            }
            if agent.client().note().is_some()
                && let Some(note) = agent.client().take_note()
            {
                crate::notify::say(note);
                if let Some(session) = session.as_ref() {
                    crate::files::refresh_from_disk(session.workspace());
                }
            }
        })
    };
    on_cleanup_local(move || drop(noting));

    view! {
        AgentSidebar(node = sidebar)
        AgentResize()
        AgentBody(composer = composer, chat = chat, review = review)
        // Over both faces: committing agent work must not need the agent view on screen.
        CommitModal(subject = commit)
        WorkflowModal(subject = workflow)
    }
}

/// The chat view, mounted beside the editor area.
///
/// The window buttons are the editor's own, handed in so they hold the same corner in both
/// faces of the window.
#[component]
fn AgentBody(
    /// The composer's editor element, for its focus sink.
    composer: NodeRef,
    /// The timeline's node, for the chat's focus sink.
    chat: NodeRef,
    /// The review surface's node, for its focus sink.
    review: NodeRef,
) -> impl IntoView {
    use zdt_view::Erase;

    let controls = view! { WindowControls() }.any();
    view! {
        AgentView(composer = composer, chat = chat, review = review, controls = controls)
    }
}
