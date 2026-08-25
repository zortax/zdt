//! What the window contains.
//!
//! Three rows inside the frame: the combined header, the panes, and the status line.
//!
//! What is built here is one window's worth: the announcements, the modal surfaces, and the view
//! tree. What the whole application shares is [`global`], built above every window. What one
//! directory's work is made of is [`crate::session`].

pub mod chrome;
pub mod frame;
pub mod global;
pub mod statusline;
pub mod theme;
pub mod window;

use std::path::PathBuf;

use zgui::prelude::*;
use zgui::reactive::RenderEffect;
use zgui::{component, view};
use zgui_ui::prelude::{ToastCorner, ToasterProps};

use crate::agent::view::AgentMountProps;
use crate::app::chrome::ChromeProps;
use crate::app::frame::FrameProps;
use crate::app::statusline::StatusLineProps;
use crate::app::theme::ZdtThemeProps;
use crate::cmdline::view::CommandLineProps;
use crate::completion::view::CompletionPopupProps;
use crate::explorer::Explorer;
use crate::explorer::drag::ghost::TreeGhostProps;
use crate::explorer::field::view::TreeFieldProps;
use crate::explorer::menu::TreeMenuProps;
use crate::explorer::tree::{ExplorerProps, TreeResizeProps};
use crate::git::Git;
use crate::hover::{Hover, HoverPanelProps};
use crate::language::Language;
use crate::picker::Picker;
use crate::picker::view::PickerProps;
use crate::prompt::Prompt;
use crate::prompt::view::PromptProps;
use crate::rename::RenameBoxProps;
use crate::session::Session;
use crate::settings::Settings;
use crate::settings::view::ConfigModalProps;
use crate::tabpick::TabPick;
use crate::terminals::view::FloatingTerminalProps;
use crate::vim::Vim;
use crate::vim::whichkey::WhichKeyProps;
use crate::workspace::panes::PanesProps;
use crate::workspace::{BufferId, Workspace};
use zdt_gitui::GitModalProps;

/// The application, as one window shows it.
///
/// Two components and not one. Announcements are found by looking *up* the scope tree, so
/// anything that announces has to sit inside the toaster rather than beside it.
#[component]
pub fn Root(
    /// Which session this window opens on.
    session: Session,
    /// The files named on the command line.
    files: Vec<PathBuf>,
) -> impl IntoView {
    let global = global::use_global();
    let (theme, scheme) = (global.theme(), global.scheme());
    view! {
        ZdtTheme(theme = theme, scheme = scheme) {
            Toaster(corner = ToastCorner::BottomRight, limit = 4, label = "Notifications") {
                WindowBody(session = session, files = files)
            }
        }
    }
}

/// One window: its announcements, its sheets, and the sessions it holds.
// The list macro takes a closure by construction, so the one it is handed here is not redundant.
#[allow(clippy::redundant_closure)]
#[component]
fn WindowBody(
    /// Which session this window opens on.
    session: Session,
    /// The files named on the command line.
    files: Vec<PathBuf>,
) -> impl IntoView {
    let mut global = global::use_global();
    let settings = global.settings().clone();
    let host = crate::session::host::use_host();

    // Before anything that might announce something, and inside the toaster, which is the only
    // place a queue can be found.
    let notify = crate::notify::Notify::new(settings.clone());
    crate::notify::provide(notify.clone());
    settings.announce_through(notify.clone());
    settings.clock().bind_here();

    // The registry's clock, and the settings', follow whichever window is open. Both outlive any
    // one of them, so every window claims them again whenever the set of windows changes: a
    // window closing must not leave a repeating job armed against an engine that has stopped.
    let lending = {
        let (settings, host) = (settings.clone(), host.clone());
        let windows = zgui::reactive::use_local_context::<zgui::runtime::windows::Windows>();
        RenderEffect::new(move |_| {
            if let Some(windows) = windows.as_ref() {
                let _ = windows.watch().get();
            }
            settings.clock().bind_here();
            host.clock().bind_here();
        })
    };
    on_cleanup_local(move || drop(lending));

    // The two sheets that belong to a document rather than to the application.
    global::install_window_styles(&global);

    if let Some(problem) = global.take_problem() {
        notify.fail("config.toml did not read", Some(problem));
    }

    // This window, as the registry knows it. It shows the session it was opened on.
    let client = host.register_client(zgui::runtime::windows::try_use_window());
    client.show(session.id());
    crate::session::client::provide(client.clone());

    for file in files {
        crate::files::open_argument(session.workspace(), &file);
    }

    // What this window is called follows what it is showing.
    let titling = {
        let (host, client) = (host.clone(), client.clone());
        RenderEffect::new(move |_| {
            let Some(showing) = client.showing() else {
                return;
            };
            if let Some(session) = host.session(showing) {
                client.set_title(&window::title_for(&session.name()));
            }
        })
    };
    on_cleanup_local(move || drop(titling));

    // What a window closing does: every session it held is written down before its clock goes.
    let closing = {
        let host = host.clone();
        zgui::runtime::windows::on_close_request(move || {
            host.flush_all();
            zgui::runtime::CloseResponse::Close
        })
    };
    on_cleanup_local(move || drop(closing));

    {
        let (settings, host, id) = (settings.clone(), host.clone(), client.id());
        on_cleanup_local(move || {
            // The stack goes with this window; the clocks do not, because another window may be
            // about to claim them. A clock with a dead engine is re-armed by the effect above.
            settings.announcer().unbind();
            host.forget_client(id);
        });
    }

    // One subtree per session this window holds. All but one are taken out of the flow rather
    // than unmounted, because unmounting a session stops its programs.
    let shells = {
        let (host, client) = (host.clone(), client.clone());
        move || -> Vec<Session> {
            client
                .held()
                .into_iter()
                .filter_map(|id| host.session(id))
                .collect()
        }
    };

    view! {
        box(class = "client") {
            for session in move || shells(), key = |session: &Session| session.id() {
                SessionShell(session = session, notify = notify.clone())
            }
        }
    }
}

/// One session, as one window draws it.
///
/// Everything the session owns was built long before this and is only republished here. What is
/// built here is the window's own: the modal surfaces, which have no meaning shared between two
/// windows over one session.
#[component]
fn SessionShell(
    /// Which session this draws.
    session: Session,
    /// This window's announcements.
    notify: crate::notify::Notify,
) -> impl IntoView {
    let global = global::use_global();
    let settings = global.settings().clone();
    let client = crate::session::client::use_client();

    // The session borrows this window's clock and stack for as long as this subtree is mounted.
    let attachment = session.attach(client.id(), notify);
    on_cleanup_local(move || drop(attachment));

    // Everything the session owns.
    session.provide();

    // Whether this is the session on screen. A window keeps several mounted and takes all but one
    // out of the flow, so this decides both what is drawn and who may hold the keyboard.
    let showing = {
        let (client, id) = (client.clone(), session.id());
        Signal::derive_local(move || client.showing() == Some(id))
    };

    // The one thing in the application that gives a node the keyboard. Every region says how it
    // takes it and none of them takes it for itself, so two regions cannot arm two claims in one
    // flush and leave the later one to win.
    let projection =
        crate::focus::project::project(session.workspace().focus(), session.workspace(), showing);
    on_cleanup_local(move || drop(projection));

    // And everything this window owns over it.
    crate::prompt::provide(Prompt::new());
    crate::explorer::menu::provide();
    crate::explorer::field::provide(crate::explorer::field::Field::new());
    crate::hover::provide(Hover::new());
    crate::rename::provide(crate::rename::Rename::new());
    crate::tabpick::provide(TabPick::new(session.workspace().clone()));
    crate::picker::provide(Picker::new(session.workspace().clone(), settings.clone()));
    crate::completion::provide(crate::completion::Completion::new(
        settings.clone(),
        Some(session.language().clone()),
    ));
    crate::settings::view::provide(crate::settings::view::ConfigModalState::new());

    // A session that has never been worked in takes the panel's visibility from the settings. One
    // that has takes it from what it wrote down, which `restore` has already put back.
    //
    // The visibility alone. A panel that opens because a setting says so is not a panel somebody
    // asked to type in.
    if settings.with_untracked(|config| config.tree.open)
        && let Some(explorer) = zgui::reactive::use_local_context::<Explorer>()
    {
        explorer.set_open(true);
    }

    // The git panel, floating: an overlay over whatever had the keyboard, and where the keyboard
    // lands while it is up.
    let git_panel = NodeRef::new();
    {
        let gitui = zdt_gitui::use_gitui();
        crate::focus::claim::claim(
            crate::focus::Overlay::GitModal,
            Signal::derive_local(move || gitui.is_open()),
        );
    }
    crate::focus::claim::sink(
        crate::focus::Spot::Overlay(crate::focus::Overlay::GitModal),
        crate::focus::Sink::Node(git_panel),
    );

    // Which face the window shows. Both stay mounted; the one not showing is out of the flow,
    // exactly as hidden sessions are.
    let editing = {
        let agent = zdt_agentui::use_agent();
        move || (agent.screen() == zdt_agentui::Screen::Agent).then(|| "none".to_owned())
    };

    view! {
        box(
            class = "session",
            style:display = move || (!showing.get()).then(|| "none".to_owned())
        ) {
            Frame {
                // The tree runs the whole height of the window and the buffer line sits over the
                // panes alone: a tab bar reaching across a file tree says the tabs belong to the
                // tree, and they do not. The agent surface sits left of them all, and its chat
                // takes the editor area's place when it is the screen.
                row(class = "frame__body") {
                    AgentMount(showing = showing)
                    row(class = "editorarea", style:display = editing) {
                        Explorer()
                        TreeResize()
                        column(class = "workarea") {
                            Chrome()
                            Panes()
                        }
                    }
                }
                HoverPanel()
                CompletionPopup()
                RenameBox()
                GitModal(element_ref = git_panel)
                ConfigModal()
                TreeMenu()
                TreeField()
                TreeGhost()
                FloatingTerminal()
                Picker()
                Prompt()
                WhichKey()
                CommandLine()
                StatusLine()
            }
        }
    }
}

/// Tells the language layer about every file that is opened.
///
/// This watches the buffer list. A buffer arrives from the picker, the tree, the command line and
/// the command line arguments. One place sees all four.
pub(crate) fn follow_buffers(
    language: &Language,
    workspace: &Workspace,
    git: &Git,
) -> RenderEffect<Vec<BufferId>> {
    let (language, workspace, git) = (language.clone(), workspace.clone(), git.clone());
    RenderEffect::new(move |previous: Option<Vec<BufferId>>| {
        let order = workspace.order();
        let previous = previous.unwrap_or_default();

        for id in &order {
            if !previous.contains(id) {
                language.opened(*id);
                git.refresh(*id);
            }
        }
        for id in &previous {
            if !order.contains(id)
                && let Some(path) = workspace
                    .buffer_untracked(*id)
                    .and_then(|buffer| buffer.path)
            {
                language.closed(&path);
            }
        }
        order
    })
}

/// Moves the tree's caret onto whatever the editor is showing.
///
/// Only while the panel is open, because opening the way to a file reads every directory along it
/// and there is no reason to pay that for a panel nobody is looking at.
pub(crate) fn follow_buffer(
    explorer: &Explorer,
    space: &Workspace,
    settings: &Settings,
) -> RenderEffect<()> {
    let (explorer, space, settings) = (explorer.clone(), space.clone(), settings.clone());
    RenderEffect::new(move |_| {
        let path = space.current_buffer().and_then(|buffer| buffer.path);
        if !explorer.is_open() || !settings.with(|config| config.tree.follow) {
            return;
        }
        // Never while the keyboard is in the panel. A caret that jumps out from under somebody
        // walking the tree is worse than one that is a file behind.
        if explorer.is_focused_untracked() {
            return;
        }
        if let Some(path) = path {
            explorer.reveal(&path);
        }
    })
}

/// Keeps what the tree shows in step with the settings.
pub(crate) fn follow_filter(explorer: &Explorer, settings: &Settings) -> RenderEffect<()> {
    let (explorer, settings) = (explorer.clone(), settings.clone());
    RenderEffect::new(move |_| {
        let wanted = settings.with(|config| zdt_core::tree::Filter {
            hidden: config.tree.hidden,
            ignored: config.tree.ignored,
        });
        if explorer.filter() != wanted {
            explorer.set_filter(wanted);
        }
    })
}

/// Reads what git says about the tree while the tree is open, and stops while it is closed.
///
/// A status reads the whole working tree. Nobody is looking at a closed panel, so nothing is read
/// and nothing is watched until it opens.
pub(crate) fn follow_status(explorer: &Explorer, status: &crate::git::Status) -> RenderEffect<()> {
    let (explorer, status) = (explorer.clone(), status.clone());
    RenderEffect::new(move |_| status.watch(explorer.is_open()))
}

/// The keys leap labels are drawn from, and again whenever the settings change.
pub(crate) fn follow_alphabet(vim: &Vim, settings: &Settings) -> RenderEffect<()> {
    let (vim, settings) = (vim.clone(), settings.clone());
    RenderEffect::new(move |_| {
        let alphabet = settings.with(|config| config.leap.alphabet.clone());
        vim.leaping().set_alphabet(&alphabet);
    })
}
