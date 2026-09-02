//! Sessions: one directory's editor state, and what names it.
//!
//! A session is a directory. Opening a directory attaches to its session, and the editor is
//! always in one. Sessions follow tmux: a session outlives the window looking at it, and the
//! same directory is never two sessions.
//!
//! # The three tiers
//!
//! A session is the middle one. Above it is [`crate::app::global`], which every session shares.
//! Below it is the window, which is one view of one session and is rebuilt whenever a window
//! attaches. What is here is one directory's work: its buffers, its splits, its terminals, its
//! servers and its undo history.
//!
//! Everything a session owns is made under [`Session`]'s own reactive owner, which is a child of
//! the application's scope and not of any window's. That is what lets a window close without
//! taking the work with it.
//!
//! [`schema`] is what a session is written down as, [`store`] is where, and [`host`] is the
//! registry of them all.

pub mod capture;
pub mod client;
pub mod host;
pub mod pick;
pub mod restore;
pub mod save;
pub mod schema;
pub mod serve;
pub mod store;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use zdt_core::Project;
use zgui::reactive::Owner;

use crate::app::global::Global;
use crate::cmdline::CommandLine;
use crate::explorer::Explorer;
use crate::git::Git;
use crate::language::Language;
use crate::notify::{Announcer, Notify};
use crate::session::client::ClientId;
use crate::terminals::Terminals;
use crate::vim::Vim;
use crate::workspace::Workspace;

slotmap::new_key_type! {
    /// Names one session for as long as the application runs.
    pub struct SessionId;
}

/// What names a session.
///
/// An enum with one variant, because a session on another machine is a second variant and every
/// comparison in the registry has to keep working when it arrives.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum SessionKey {
    /// A directory on this machine, canonical.
    Local(PathBuf),
}

impl SessionKey {
    /// The key for `dir`, or nothing when it is not a directory.
    ///
    /// Canonical, so that a relative path, a trailing separator and a symbolic link all reach the
    /// same session. Two names for one directory must not be two sessions over one set of files.
    #[must_use]
    pub fn of(dir: &Path) -> Option<Self> {
        let real = std::fs::canonicalize(dir).ok()?;
        real.is_dir().then_some(Self::Local(real))
    }

    /// The directory, for a key that names one on this machine.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Local(path) => Some(path),
        }
    }

    /// What to call it, for a picker row and a title bar.
    #[must_use]
    pub fn name(&self) -> String {
        match self {
            Self::Local(path) => path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned()),
        }
    }
}

impl std::fmt::Display for SessionKey {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local(path) => write!(out, "{}", path.display()),
        }
    }
}

/// One directory's work.
///
/// Cloning one is cloning a handle: every clone is the same session.
#[derive(Clone)]
pub struct Session {
    inner: Rc<Inner>,
}

struct Inner {
    id: SessionId,
    key: SessionKey,
    project: Project,
    /// Everything below was made under this, and dies only when the session is killed.
    ///
    /// A child of the application's scope, and never of a window's. A signal made under a window
    /// dies with it, and a session that lost its buffers when somebody closed a window would not
    /// be a session.
    owner: Owner,
    /// The clock every debounce here runs on. An attached window lends it its own.
    clock: zdt_view::Clock,
    /// Where anything this session says goes. An attached window lends it its stack.
    announcer: Announcer,
    /// Which window is looking, when one is.
    attached: RefCell<Option<ClientId>>,
    /// Where this session is written down, when there is anywhere to write it.
    state: Option<zdt_core::state::State>,

    workspace: Workspace,
    vim: Vim,
    explorer: Explorer,
    terminals: Terminals,
    language: Language,
    git: Git,
    status: crate::git::Status,
    head: crate::git::Head,
    gitui: zdt_gitui::GitUi,
    /// What keeps the buffers, the tree and the marks in step with the files on disk.
    ///
    /// Held for the session's life; dropping it stops the watching.
    disk: RefCell<Option<crate::disk::Disk>>,
    cmdline: CommandLine,
    /// The long-lived effects. Held for the session's life; dropping them stops the following.
    effects: RefCell<Vec<Box<dyn std::any::Any>>>,
    /// What writes this session down, and when.
    writer: RefCell<Option<Rc<crate::session::save::Writer>>>,
    /// The agent surface as this session's window last showed it.
    agent: RefCell<crate::session::schema::AgentSnapshot>,
    /// Where each editor was looking, including editors that have gone away.
    views: Rc<RefCell<crate::session::capture::Views>>,
}

impl Session {
    /// Builds the session for `key`, under `owner`.
    ///
    /// The order here is load-bearing and is the order the editor was built in before sessions
    /// existed: the workspace before anything that reads it, the servers before the suggestions
    /// that hold one, and the git panel after the layer that answers its keys.
    pub(crate) fn build(id: SessionId, key: SessionKey, global: &Global, owner: Owner) -> Self {
        let project = key
            .path()
            .map_or_else(|| Project::at("."), Project::session);
        let clock = zdt_view::Clock::new();
        let announcer = Announcer::new();

        let inner = owner.with(|| {
            let workspace = Workspace::new(project.clone());
            let vim = Vim::new(
                workspace.clone(),
                global.settings().clone(),
                global.keymaps().clone(),
            );
            let explorer = Explorer::new(
                project.root().to_path_buf(),
                global.tree_filter(),
                workspace.focus().clone(),
            );
            let terminals = Terminals::new(workspace.clone(), global.settings().clone());
            let cmdline = CommandLine::new(workspace.clone());

            // The language servers. Nothing starts until a file that wants one is opened.
            let language = Language::new(
                workspace.clone(),
                global.settings().clone(),
                clock.clone(),
                announcer.clone(),
            );
            language.listen();

            // What git says about the open files, what it says about every path in the tree, and
            // the panel that shows the rest of it.
            let git = Git::new(workspace.clone(), clock.clone());
            let status = crate::git::Status::new(project.root().to_path_buf(), clock.clone());
            let head =
                crate::git::Head::new(project.tooling_root().to_path_buf(), project.git_branch());
            let gitui = crate::git::panel(
                workspace.clone(),
                vim.clone(),
                announcer.clone(),
                status.clone(),
            );

            Inner {
                id,
                key,
                project,
                owner: owner.clone(),
                clock: clock.clone(),
                announcer: announcer.clone(),
                attached: RefCell::new(None),
                state: zdt_core::state::State::discover(),
                workspace,
                vim,
                explorer,
                terminals,
                language,
                git,
                status,
                head,
                gitui,
                disk: RefCell::new(None),
                cmdline,
                effects: RefCell::new(Vec::new()),
                writer: RefCell::new(None),
                agent: RefCell::new(crate::session::schema::AgentSnapshot::default()),
                views: Rc::new(RefCell::new(crate::session::capture::Views::default())),
            }
        });

        let session = Self {
            inner: Rc::new(inner),
        };

        // What was written down last time, put back before anything is drawn: the first frame has
        // to be the right one, and a layout that arrives on frame three is a difference somebody
        // notices.
        let held = crate::session::save::read_for(session.key());
        let generation = held.as_ref().map_or(0, |snapshot| snapshot.generation);
        if let Some(snapshot) = held.as_ref() {
            // Kept rather than applied: the agent surface is above every session, and the one
            // startup restore reads this back through `agent_view`.
            *session.inner.agent.borrow_mut() = snapshot.agent.clone();
            // The map is taken out rather than borrowed: putting a session back opens buffers,
            // and a buffer's editor records where it is looking through this same cache.
            let mut views = crate::session::capture::Views::default();
            let report = crate::session::restore::apply(&session, snapshot, &mut views);
            session.inner.views.borrow_mut().extend(views);
            if let Some(said) = report.say() {
                announcer.warn(said);
            }
        }

        *session.inner.writer.borrow_mut() = Some(crate::session::save::Writer::new(
            session.clone(),
            generation,
            Rc::clone(&session.inner.views),
            held,
        ));

        session.follow(global);
        session
    }

    /// Starts the three effects that keep the session's parts in step with each other.
    ///
    /// Made under the session's owner, so they live as long as what they are following.
    fn follow(&self, global: &Global) {
        let inner = &self.inner;
        let settings = global.settings();
        let effects: Vec<Box<dyn std::any::Any>> = inner.owner.with(|| {
            vec![
                Box::new(crate::app::follow_buffers(
                    &inner.language,
                    &inner.workspace,
                    &inner.git,
                )) as Box<dyn std::any::Any>,
                Box::new(crate::app::follow_buffer(
                    &inner.explorer,
                    &inner.workspace,
                    settings,
                )),
                Box::new(crate::app::follow_filter(&inner.explorer, settings)),
                Box::new(crate::app::follow_tree_width(&inner.explorer, settings)),
                Box::new(crate::app::follow_status(&inner.explorer, &inner.status)),
                Box::new(crate::app::follow_alphabet(&inner.vim, settings)),
                // Where every editor was looking, put back as each one mounts.
                Box::new(crate::session::restore::follow_mounts(
                    self,
                    Rc::clone(&inner.views),
                )),
                // And what says the arrangement moved.
                Box::new(follow_structure(self)),
            ]
        });
        *inner.effects.borrow_mut() = effects;

        // And the watch on the project. Made under the session's owner too, because what it reads
        // into is everything above.
        let disk = inner.owner.with(|| {
            crate::disk::Disk::follow(
                &inner.workspace,
                &inner.explorer,
                &inner.git,
                &inner.status,
                &inner.head,
                &inner.clock,
            )
        });
        *inner.disk.borrow_mut() = disk;
    }

    /// Which session this is.
    #[must_use]
    pub fn id(&self) -> SessionId {
        self.inner.id
    }

    /// What names it.
    #[must_use]
    pub fn key(&self) -> &SessionKey {
        &self.inner.key
    }

    /// The directory it was opened on, and the one its tooling is rooted at.
    #[must_use]
    pub fn project(&self) -> &Project {
        &self.inner.project
    }

    /// What it is called, for a title bar and a picker row.
    #[must_use]
    pub fn name(&self) -> String {
        self.inner.key.name()
    }

    /// The buffers, the splits and the layout.
    #[must_use]
    pub fn workspace(&self) -> &Workspace {
        &self.inner.workspace
    }

    /// The modal layer.
    #[must_use]
    pub fn vim(&self) -> &Vim {
        &self.inner.vim
    }

    /// The language servers.
    #[must_use]
    pub fn language(&self) -> &Language {
        &self.inner.language
    }

    /// The file tree.
    #[must_use]
    pub fn explorer(&self) -> &Explorer {
        &self.inner.explorer
    }

    /// The terminals.
    #[must_use]
    pub fn terminals(&self) -> &Terminals {
        &self.inner.terminals
    }

    /// The command line.
    #[must_use]
    pub fn cmdline(&self) -> &CommandLine {
        &self.inner.cmdline
    }

    /// What writes this session down.
    #[must_use]
    pub fn writer(&self) -> Option<Rc<crate::session::save::Writer>> {
        self.inner.writer.borrow().clone()
    }

    /// Says something worth writing down changed.
    pub fn touched(&self) {
        if let Some(writer) = self.writer() {
            writer.touched();
        }
    }

    /// Says one buffer's text changed.
    pub fn touched_text(&self, buffer: crate::workspace::BufferId) {
        if let Some(writer) = self.writer() {
            writer.touched_text(buffer);
        }
    }

    /// The agent surface as this session's window last showed it.
    #[must_use]
    pub fn agent_view(&self) -> crate::session::schema::AgentSnapshot {
        self.inner.agent.borrow().clone()
    }

    /// Remembers what the agent surface shows, and says it is worth writing down.
    pub fn set_agent_view(&self, view: crate::session::schema::AgentSnapshot) {
        if *self.inner.agent.borrow() == view {
            return;
        }
        *self.inner.agent.borrow_mut() = view;
        self.touched();
    }

    /// Writes whatever is owed, now. What closing a window and quitting both do.
    pub fn flush(&self) {
        if let Some(writer) = self.writer() {
            writer.flush();
        }
    }

    /// Where sessions are written down, when there is anywhere.
    #[must_use]
    pub fn state(&self) -> Option<zdt_core::state::State> {
        self.inner.state.clone()
    }

    /// The clock every debounce here runs on.
    #[must_use]
    pub fn clock(&self) -> &zdt_view::Clock {
        &self.inner.clock
    }

    /// Where anything this session says goes.
    #[must_use]
    pub fn announcer(&self) -> &Announcer {
        &self.inner.announcer
    }

    /// Whether a window is looking at this session.
    #[must_use]
    pub fn is_attached(&self) -> bool {
        self.inner.attached.borrow().is_some()
    }

    /// Which window is looking, when one is.
    #[must_use]
    pub fn attached_to(&self) -> Option<ClientId> {
        *self.inner.attached.borrow()
    }

    /// Lends this session the calling window's clock and announcements.
    ///
    /// Called from the session's own shell, inside the window, where there certainly is a clock.
    /// The answer is a guard: dropping it takes both back.
    #[must_use]
    pub fn attach(&self, client: ClientId, notify: Notify) -> Attachment {
        debug_assert!(
            self.inner.attached.borrow().is_none(),
            "a session is looked at by one window at a time",
        );
        self.inner.clock.bind_here();
        self.inner.announcer.bind(notify);
        *self.inner.attached.borrow_mut() = Some(client);
        Attachment {
            session: self.clone(),
        }
    }

    /// Publishes everything this session owns into the calling subtree.
    ///
    /// One place says what a session is, which is what stops a component reaching a neighbouring
    /// session's state by accident.
    pub fn provide(&self) {
        crate::workspace::provide(self.inner.workspace.clone());
        crate::focus::provide(self.inner.workspace.focus().clone());
        zgui::reactive::provide_local_context(self.inner.vim.clone());
        crate::explorer::provide(self.inner.explorer.clone());
        crate::terminals::provide(self.inner.terminals.clone());
        crate::language::provide(self.inner.language.clone());
        crate::git::provide(self.inner.git.clone());
        crate::git::status::provide(self.inner.status.clone());
        crate::git::head::provide(self.inner.head.clone());
        zdt_gitui::provide(self.inner.gitui.clone());
        crate::cmdline::provide(self.inner.cmdline.clone());
        zgui::reactive::provide_local_context(self.clone());
    }

    /// Everything this session holds goes, which stops its servers and its programs.
    pub(crate) fn dispose(&self) {
        self.inner.disk.borrow_mut().take();
        self.inner.effects.borrow_mut().clear();
        self.inner.owner.cleanup();
    }
}

/// Says the session should be written down whenever its shape changes.
///
/// The buffer line, the splits and which one has the keyboard: everything a manifest is made of
/// that is not the text itself.
fn follow_structure(session: &Session) -> zgui::reactive::RenderEffect<()> {
    let session = session.clone();
    zgui::reactive::RenderEffect::new(move |previous: Option<()>| {
        let workspace = session.workspace();
        let _ = workspace.order();
        let _ = workspace.shape();
        let _ = workspace.focused();
        let _ = workspace.mounted_revision();
        workspace.track_windows();
        // The first run is the state that was just restored, which is already on disk.
        if previous.is_some() {
            session.touched();
        }
    })
}

/// A window's loan of its clock and its announcements to a session.
///
/// Dropping it takes both back, which is what a window closing does.
pub struct Attachment {
    session: Session,
}

impl Drop for Attachment {
    fn drop(&mut self) {
        self.session.inner.clock.unbind();
        self.session.inner.announcer.unbind();
        self.session.inner.attached.borrow_mut().take();
    }
}

/// The session this component is inside, when it is inside one.
#[must_use]
pub fn use_session() -> Option<Session> {
    zgui::reactive::use_local_context::<Session>()
}

#[cfg(test)]
mod tests {
    use super::SessionKey;

    #[test]
    fn a_file_is_not_a_session() {
        let file = std::env::temp_dir().join(format!("zdt-key-{}", std::process::id()));
        std::fs::write(&file, "").expect("the file is written");
        assert_eq!(SessionKey::of(&file), None);
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn a_directory_that_is_not_there_is_not_a_session() {
        assert_eq!(
            SessionKey::of(std::path::Path::new("/nowhere/at/all")),
            None
        );
    }

    #[test]
    fn two_names_for_one_directory_are_one_key() {
        let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let roundabout = here.join("src").join("..");
        assert_eq!(SessionKey::of(here), SessionKey::of(&roundabout));
    }

    #[test]
    fn the_name_is_the_last_component() {
        let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        assert_eq!(SessionKey::of(here).expect("a directory").name(), "zdt");
    }
}
