//! The daemon, as the interface reads it.
//!
//! One connection to `zdt-agentd`, held open for the application's life and reconnected when it
//! drops. What the daemon pushes lands in signals; what the interface asks goes out through a
//! channel. Everything runs on the interface thread — the runtime behind `zgui::tokio` polls
//! local tasks inside its own context, so the socket is awaited right where the signals live.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use zdt_agent::ask::{Ask, Decision};
use zdt_agent::catalog::Catalog;
use zdt_agent::mode::RuntimeMode;
use zdt_agent::protocol::{ClientMsg, ServerMsg};
use zdt_agent::thread::{ItemKind, ThreadId, ThreadShell, ThreadState, TimelineItem};
use zdt_agent::todo::Todo;
use zdt_agent::{VERSION, wire};
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

/// Something that happened to a thread nobody was looking at.
#[derive(Clone, Debug)]
pub enum Notice {
    /// A turn finished.
    Done {
        /// Which thread.
        thread: ThreadId,
        /// What it is called.
        title: String,
    },
    /// A turn broke.
    Failed {
        /// Which thread.
        thread: ThreadId,
        /// What it is called.
        title: String,
        /// What went wrong.
        error: String,
    },
    /// A turn stopped to ask something.
    Asking {
        /// Which thread.
        thread: ThreadId,
        /// What it is called.
        title: String,
    },
}

impl Notice {
    /// Which thread the notice is about.
    #[must_use]
    pub fn thread(&self) -> ThreadId {
        match self {
            Self::Done { thread, .. }
            | Self::Failed { thread, .. }
            | Self::Asking { thread, .. } => *thread,
        }
    }
}

/// A drafted commit message, as the daemon wrote it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitDraft {
    /// The directory it was drafted for.
    pub root: PathBuf,
    /// One imperative line.
    pub subject: String,
    /// The body under it.
    pub body: String,
    /// A short branch name for the change.
    pub branch: String,
}

/// How long to wait after a failed connection before the next try.
const RETRY_FLOOR: Duration = Duration::from_millis(500);

/// The longest the retry pause grows to.
const RETRY_CEILING: Duration = Duration::from_secs(5);

/// How long a freshly spawned daemon is given before spawning is considered again.
const SPAWN_PATIENCE: Duration = Duration::from_secs(3);

/// The daemon, as a handle.
///
/// Cloning one is cloning a handle: every clone reads the same connection.
#[derive(Clone)]
pub struct AgentClient {
    inner: Rc<Inner>,
}

struct Inner {
    /// Whether the daemon is on the other end right now.
    connected: RwSignal<bool, LocalStorage>,
    /// Whether the daemon has said what threads there are, at least once.
    ///
    /// An empty list before it speaks says nothing, and something that acts on "there are no
    /// threads here" must wait for the difference.
    listed: RwSignal<bool, LocalStorage>,
    /// Every thread, newest first, as the daemon last said.
    threads: RwSignal<Vec<ThreadShell>, LocalStorage>,
    /// Which thread's conversation is followed.
    watching: RwSignal<Option<ThreadId>, LocalStorage>,
    /// The followed thread's row order, oldest first.
    ///
    /// Structure apart from content: a delta grows one row's own signal, so a streaming word
    /// wakes that row and nothing else.
    order: RwSignal<Vec<i64>, LocalStorage>,
    /// Each row's content, under its id.
    rows: RefCell<std::collections::HashMap<i64, RwSignal<TimelineItem, LocalStorage>>>,
    /// What the followed thread stops to ask, oldest first.
    asks: RwSignal<Vec<Ask>, LocalStorage>,
    /// What the watched thread runs beside its main agent.
    runners: RwSignal<Vec<zdt_agent::runner::Runner>, LocalStorage>,
    /// The followed thread's proposed plan, while one waits.
    plan: RwSignal<Option<String>, LocalStorage>,
    /// The followed thread's checklist.
    todos: RwSignal<Vec<Todo>, LocalStorage>,
    /// What the followed thread's session offers.
    catalog: RwSignal<Catalog, LocalStorage>,
    /// What happened to threads since somebody last asked, oldest first.
    news: RwSignal<Vec<Notice>, LocalStorage>,
    /// The thread the last `create` made, waiting to be taken.
    created: RwSignal<Option<ThreadId>, LocalStorage>,
    /// The last thing that went wrong, waiting to be announced.
    problem: RwSignal<Option<String>, LocalStorage>,
    /// The last thing that went well and is worth a line, waiting to be said.
    note: RwSignal<Option<String>, LocalStorage>,
    /// Why there is no connection, while there is none.
    ///
    /// A standing line for the sidebar's foot, apart from [`problem`](Self::problem): a daemon
    /// that cannot be started says so there, quietly and continuously, rather than as a toast
    /// every retry.
    standing: RwSignal<Option<String>, LocalStorage>,
    /// What the last content search turned up: the words, and the threads that have them.
    found: RwSignal<Option<(String, Vec<zdt_agent::protocol::FoundRow>)>, LocalStorage>,
    /// What the last import scan turned up: the instance, and its conversations.
    imports: RwSignal<Option<(String, Vec<zdt_agent::protocol::ImportRow>)>, LocalStorage>,
    /// What a commit of the scanned thread would take, from the last scan.
    commit_files: RwSignal<Option<(PathBuf, Vec<zdt_agent::change::FileStat>)>, LocalStorage>,
    /// The drafted commit message, once the model has written.
    commit_draft: RwSignal<Option<CommitDraft>, LocalStorage>,
    /// Where commands go while a connection is up.
    outbox: RefCell<Option<UnboundedSender<ClientMsg>>>,
}

impl AgentClient {
    /// A client that is not connected yet, already trying.
    ///
    /// Made in the application's scope, above every window, so the connection outlives them all.
    #[must_use]
    pub fn install() -> Self {
        let client = Self {
            inner: Rc::new(Inner {
                connected: RwSignal::new_local(false),
                listed: RwSignal::new_local(false),
                threads: RwSignal::new_local(Vec::new()),
                watching: RwSignal::new_local(None),
                order: RwSignal::new_local(Vec::new()),
                rows: RefCell::new(std::collections::HashMap::new()),
                asks: RwSignal::new_local(Vec::new()),
                runners: RwSignal::new_local(Vec::new()),
                plan: RwSignal::new_local(None),
                todos: RwSignal::new_local(Vec::new()),
                catalog: RwSignal::new_local(Catalog::default()),
                news: RwSignal::new_local(Vec::new()),
                created: RwSignal::new_local(None),
                problem: RwSignal::new_local(None),
                note: RwSignal::new_local(None),
                standing: RwSignal::new_local(None),
                found: RwSignal::new_local(None),
                imports: RwSignal::new_local(None),
                commit_files: RwSignal::new_local(None),
                commit_draft: RwSignal::new_local(None),
                outbox: RefCell::new(None),
            }),
        };
        let maintaining = client.clone();
        zdt_view::detached(async move { maintaining.maintain().await });
        client
    }

    // ---- What the interface reads ------------------------------------------------------------

    /// Whether the daemon is there. Tracked.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.inner.connected.get()
    }

    /// Whether the daemon has said what threads there are. Tracked.
    ///
    /// Until it has, an empty [`threads`](Self::threads) means "not answered yet" and never
    /// "there are none".
    #[must_use]
    pub fn has_listed(&self) -> bool {
        self.inner.listed.get()
    }

    /// The same, without subscribing.
    #[must_use]
    pub fn has_listed_untracked(&self) -> bool {
        self.inner.listed.get_untracked()
    }

    /// Every thread, newest first. Tracked.
    #[must_use]
    pub fn threads(&self) -> Vec<ThreadShell> {
        self.inner.threads.get()
    }

    /// Whether the daemon has said there is at least one thread. Tracked.
    #[must_use]
    pub fn has_threads(&self) -> bool {
        self.inner.threads.with(|threads| !threads.is_empty())
    }

    /// The same, without subscribing.
    #[must_use]
    pub fn threads_untracked(&self) -> Vec<ThreadShell> {
        self.inner.threads.get_untracked()
    }

    /// One thread's shell, when the daemon has said. Tracked.
    #[must_use]
    pub fn thread(&self, thread: ThreadId) -> Option<ThreadShell> {
        self.inner
            .threads
            .with(|threads| threads.iter().find(|shell| shell.id == thread).cloned())
    }

    /// Which thread's conversation is followed. Tracked.
    #[must_use]
    pub fn watching(&self) -> Option<ThreadId> {
        self.inner.watching.get()
    }

    /// The followed thread's row order, oldest first. Tracked.
    #[must_use]
    pub fn order(&self) -> Vec<i64> {
        self.inner.order.get()
    }

    /// One row's own signal, when the row is there.
    #[must_use]
    pub fn row(&self, id: i64) -> Option<RwSignal<TimelineItem, LocalStorage>> {
        self.inner.rows.borrow().get(&id).copied()
    }

    /// The followed thread's rows, oldest first. Tracked through the order.
    #[must_use]
    pub fn items(&self) -> Vec<TimelineItem> {
        let order = self.inner.order.get();
        let rows = self.inner.rows.borrow();
        order
            .iter()
            .filter_map(|id| rows.get(id).map(|row| row.get_untracked()))
            .collect()
    }

    /// What the followed thread stops to ask, oldest first. Tracked.
    #[must_use]
    pub fn asks(&self) -> Vec<Ask> {
        self.inner.asks.get()
    }

    /// The same, without subscribing.
    #[must_use]
    pub fn asks_untracked(&self) -> Vec<Ask> {
        self.inner.asks.get_untracked()
    }

    /// What the followed thread runs beside its main agent. Tracked.
    #[must_use]
    pub fn runners(&self) -> Vec<zdt_agent::runner::Runner> {
        self.inner.runners.get()
    }

    /// The followed thread's proposed plan, while one waits. Tracked.
    #[must_use]
    pub fn plan(&self) -> Option<String> {
        self.inner.plan.get()
    }

    /// The followed thread's checklist. Tracked.
    #[must_use]
    pub fn todos(&self) -> Vec<Todo> {
        self.inner.todos.get()
    }

    /// What the followed thread's session offers. Tracked.
    #[must_use]
    pub fn catalog(&self) -> Catalog {
        self.inner.catalog.get()
    }

    /// The same, without subscribing.
    #[must_use]
    pub fn catalog_untracked(&self) -> Catalog {
        self.inner.catalog.get_untracked()
    }

    /// What happened to threads lately. Taking it clears it.
    #[must_use]
    pub fn take_news(&self) -> Vec<Notice> {
        let held = self.inner.news.get_untracked();
        if held.is_empty() {
            return held;
        }
        self.inner.news.set(Vec::new());
        held
    }

    /// Whether there is news, without taking it. Tracked.
    #[must_use]
    pub fn has_news(&self) -> bool {
        self.inner.news.with(|held| !held.is_empty())
    }

    /// The thread the last `create` made. Taking it clears it.
    #[must_use]
    pub fn take_created(&self) -> Option<ThreadId> {
        let held = self.inner.created.get_untracked();
        if held.is_some() {
            self.inner.created.set(None);
        }
        held
    }

    /// The thread the last `create` made, without taking it. Tracked.
    #[must_use]
    pub fn created(&self) -> Option<ThreadId> {
        self.inner.created.get()
    }

    /// The last thing that went wrong. Taking it clears it.
    #[must_use]
    pub fn take_problem(&self) -> Option<String> {
        let held = self.inner.problem.get_untracked();
        if held.is_some() {
            self.inner.problem.set(None);
        }
        held
    }

    /// The last thing that went wrong, without taking it. Tracked.
    #[must_use]
    pub fn problem(&self) -> Option<String> {
        self.inner.problem.get()
    }

    /// The last thing worth a quiet line. Taking it clears it.
    #[must_use]
    pub fn take_note(&self) -> Option<String> {
        let held = self.inner.note.get_untracked();
        if held.is_some() {
            self.inner.note.set(None);
        }
        held
    }

    /// The same, without taking it. Tracked.
    #[must_use]
    pub fn note(&self) -> Option<String> {
        self.inner.note.get()
    }

    /// Why there is no connection, while there is none. Tracked.
    #[must_use]
    pub fn standing(&self) -> Option<String> {
        self.inner.standing.get()
    }

    // ---- What the interface asks -------------------------------------------------------------

    /// Makes a thread that works in `root`, on the named provider instance. Empty means the
    /// daemon's default. The answer arrives through [`Self::created`].
    pub fn create(&self, root: PathBuf, instance: String) {
        self.ask(ClientMsg::Create {
            root,
            title: String::new(),
            worktree: None,
            instance,
        });
    }

    /// Makes a thread in a worktree of its own, branched from `base` in `root`'s repository.
    pub fn create_worktree(
        &self,
        root: PathBuf,
        base: String,
        from_origin: bool,
        instance: String,
    ) {
        self.ask(ClientMsg::Create {
            root,
            title: String::new(),
            worktree: Some(zdt_agent::protocol::WorktreeSpec { base, from_origin }),
            instance,
        });
    }

    /// Puts the working tree back to before `turn` ran, and forgets that turn onward.
    pub fn revert(&self, thread: ThreadId, turn: i64) {
        self.ask(ClientMsg::Revert { thread, turn });
    }

    /// Commits the thread's whole working tree, and pushes it when asked.
    ///
    /// A non-empty `branch` is made at `HEAD` first and the commit lands on it.
    pub fn commit(
        &self,
        root: PathBuf,
        thread: Option<ThreadId>,
        message: String,
        push: bool,
        branch: String,
        paths: Vec<String>,
    ) {
        self.ask(ClientMsg::Commit {
            root,
            thread,
            message,
            push,
            branch,
            paths,
        });
    }

    /// Scans what a commit would take and has a message drafted for it.
    ///
    /// The files land in [`Self::commit_files`], the draft in [`Self::commit_draft`].
    pub fn draft_commit(&self, root: PathBuf) {
        self.inner.commit_files.set(None);
        self.inner.commit_draft.set(None);
        self.ask(ClientMsg::DraftCommit { root });
    }

    /// What a commit of the scanned thread would take. Tracked.
    #[must_use]
    pub fn commit_files(&self) -> Option<(PathBuf, Vec<zdt_agent::change::FileStat>)> {
        self.inner.commit_files.get()
    }

    /// The drafted commit message, once the model has written. Tracked.
    #[must_use]
    pub fn commit_draft(&self) -> Option<CommitDraft> {
        self.inner.commit_draft.get()
    }

    /// Sends a prompt into a thread.
    pub fn send(&self, thread: ThreadId, text: String) {
        self.ask(ClientMsg::Send { thread, text });
    }

    /// Stops the turn that is running.
    pub fn interrupt(&self, thread: ThreadId) {
        self.ask(ClientMsg::Interrupt { thread });
    }

    /// Decides an open tool ask.
    pub fn decide(&self, thread: ThreadId, id: String, decision: Decision) {
        self.ask(ClientMsg::Decide {
            thread,
            id,
            decision,
        });
    }

    /// Answers an open question ask with the chosen option labels, one list per question.
    pub fn answer(&self, thread: ThreadId, id: String, answers: Vec<Vec<String>>) {
        self.ask(ClientMsg::Answer {
            thread,
            id,
            answers,
        });
    }

    /// Takes the proposed plan and has it carried out.
    pub fn implement(&self, thread: ThreadId) {
        self.ask(ClientMsg::Implement { thread });
    }

    /// Sets how much a thread's agent may do unasked.
    pub fn set_mode(&self, thread: ThreadId, mode: RuntimeMode) {
        self.ask(ClientMsg::SetMode { thread, mode });
    }

    /// Sets which model a thread talks to. Empty means the provider's default.
    pub fn set_model(&self, thread: ThreadId, model: String) {
        self.ask(ClientMsg::SetModel { thread, model });
    }

    /// Sets how hard a thread's agent reasons. Empty means the provider's default.
    pub fn set_effort(&self, thread: ThreadId, effort: String) {
        self.ask(ClientMsg::SetEffort { thread, effort });
    }

    /// Follows one thread's conversation.
    pub fn watch(&self, thread: ThreadId) {
        if self.inner.watching.get_untracked() == Some(thread) {
            return;
        }
        self.inner.watching.set(Some(thread));
        self.replace_items(Vec::new());
        self.inner.asks.set(Vec::new());
        self.inner.runners.set(Vec::new());
        self.inner.plan.set(None);
        self.inner.todos.set(Vec::new());
        self.inner.catalog.set(Catalog::default());
        self.ask(ClientMsg::Watch { thread });
    }

    /// Takes the thread away, history included.
    pub fn delete(&self, thread: ThreadId) {
        if self.inner.watching.get_untracked() == Some(thread) {
            self.inner.watching.set(None);
            self.replace_items(Vec::new());
        }
        self.ask(ClientMsg::Delete { thread });
    }

    /// Gives a thread a place among the pinned ones. Zero unpins it.
    pub fn pin(&self, thread: ThreadId, order: f64) {
        self.ask(ClientMsg::Pin { thread, order });
    }

    /// Puts a thread to sleep until `until_ms`. Zero wakes it now.
    pub fn snooze(&self, thread: ThreadId, until_ms: u64) {
        self.ask(ClientMsg::Snooze { thread, until_ms });
    }

    /// Puts a thread away as done, or takes it back out.
    pub fn settle(&self, thread: ThreadId, settled: bool) {
        self.ask(ClientMsg::Settle { thread, settled });
    }

    /// Archives a thread, or brings it back.
    pub fn archive(&self, thread: ThreadId, archived: bool) {
        self.ask(ClientMsg::Archive { thread, archived });
    }

    /// Marks a thread read or unread by hand.
    pub fn mark_unread(&self, thread: ThreadId, unread: bool) {
        self.ask(ClientMsg::MarkUnread { thread, unread });
    }

    /// Calls a thread something else. Empty asks the daemon to make a title up.
    pub fn rename(&self, thread: ThreadId, title: String) {
        self.ask(ClientMsg::Rename { thread, title });
    }

    /// Keeps the prompt typed into a thread's composer and not sent yet.
    pub fn set_draft(&self, thread: ThreadId, text: String) {
        self.ask(ClientMsg::SetDraft { thread, text });
    }

    /// Looks for threads whose conversation contains the words. The answer lands in
    /// [`Self::take_found`].
    pub fn search(&self, query: String) {
        self.ask(ClientMsg::Search { query });
    }

    /// Lists the conversations an instance's provider holds on disk. The answer lands in
    /// [`Self::take_imports`].
    pub fn list_imports(&self, instance: String) {
        self.inner.imports.set(None);
        self.ask(ClientMsg::ListImports { instance });
    }

    /// Makes a thread out of one of them. The answer arrives through [`Self::created`].
    pub fn import(&self, instance: String, id: String) {
        self.ask(ClientMsg::Import { instance, id });
    }

    /// What the last import scan turned up. Taking it clears it.
    #[must_use]
    pub fn take_imports(&self) -> Option<(String, Vec<zdt_agent::protocol::ImportRow>)> {
        let held = self.inner.imports.get_untracked();
        if held.is_some() {
            self.inner.imports.set(None);
        }
        held
    }

    /// Whether an import scan has answered, without taking it. Tracked.
    #[must_use]
    pub fn has_imports(&self) -> bool {
        self.inner.imports.with(Option::is_some)
    }

    /// What the last search turned up. Taking it clears it.
    #[must_use]
    pub fn take_found(&self) -> Option<(String, Vec<zdt_agent::protocol::FoundRow>)> {
        let held = self.inner.found.get_untracked();
        if held.is_some() {
            self.inner.found.set(None);
        }
        held
    }

    /// Whether a search has answered, without taking it. Tracked.
    #[must_use]
    pub fn has_found(&self) -> bool {
        self.inner.found.with(Option::is_some)
    }

    /// Stops the daemon, running turns included.
    pub fn shutdown_daemon(&self) {
        self.ask(ClientMsg::Shutdown);
    }

    fn ask(&self, message: ClientMsg) {
        let sent = self
            .inner
            .outbox
            .borrow()
            .as_ref()
            .is_some_and(|outbox| outbox.send(message).is_ok());
        if !sent {
            self.inner
                .problem
                .set(Some("the agent daemon is not connected yet".to_owned()));
        }
    }

    // ---- The connection ----------------------------------------------------------------------

    /// Connects, reconnects, and never returns.
    async fn maintain(&self) {
        let mut pause = RETRY_FLOOR;
        let mut last_spawn: Option<std::time::Instant> = None;
        loop {
            match self.connect(&mut last_spawn).await {
                Some(stream) => {
                    pause = RETRY_FLOOR;
                    self.converse(stream).await;
                    self.inner.outbox.borrow_mut().take();
                    self.inner.connected.set(false);
                }
                None => {
                    pause = (pause * 2).min(RETRY_CEILING);
                }
            }
            tokio::time::sleep(pause).await;
        }
    }

    /// One try at reaching a daemon, spawning one when nothing answers.
    async fn connect(
        &self,
        last_spawn: &mut Option<std::time::Instant>,
    ) -> Option<tokio::net::UnixStream> {
        let Some(directory) = zdt_ipc::client::directory() else {
            self.stand("there is no runtime directory for the daemon's socket");
            return None;
        };
        let socket = directory.join("agentd.sock");
        match tokio::net::UnixStream::connect(&socket).await {
            Ok(stream) => Some(stream),
            Err(_) => {
                let cooled = last_spawn.is_none_or(|when| when.elapsed() > SPAWN_PATIENCE);
                if cooled {
                    *last_spawn = Some(std::time::Instant::now());
                    match spawn_daemon() {
                        Ok(program) => {
                            self.stand(&format!("starting {program}\u{2026}"));
                        }
                        Err(said) => self.stand(&said),
                    }
                }
                None
            }
        }
    }

    /// Says why there is no connection, once per change.
    fn stand(&self, said: &str) {
        if self
            .inner
            .standing
            .with_untracked(|held| held.as_deref() != Some(said))
        {
            self.inner.standing.set(Some(said.to_owned()));
        }
    }

    /// One connection, from hello to hangup.
    async fn converse(&self, stream: tokio::net::UnixStream) {
        let (mut reading, mut writing) = stream.into_split();

        let hello = ClientMsg::Hello {
            version: VERSION,
            pid: std::process::id(),
        };
        if wire::write(&mut writing, &hello).await.is_err() {
            return;
        }
        match wire::read::<ServerMsg>(&mut reading).await {
            Ok(ServerMsg::Welcome { version, .. }) if version == VERSION => {}
            Ok(ServerMsg::Refused { reason }) => {
                // A daemon left over from an older build. It is asked to stop in its own
                // dialect, and the retry loop starts a fresh one.
                self.stand(&format!("{reason}; restarting the daemon"));
                retire_old_daemon().await;
                return;
            }
            _ => return,
        }
        self.inner.connected.set(true);
        self.inner.standing.set(None);

        // The writer: one task per connection, gone when the outbox is replaced or the pipe
        // breaks.
        let (outbox, mut commands) = unbounded_channel::<ClientMsg>();
        *self.inner.outbox.borrow_mut() = Some(outbox);
        zdt_view::detached(async move {
            while let Some(message) = commands.recv().await {
                if wire::write(&mut writing, &message).await.is_err() {
                    return;
                }
            }
        });

        // A fresh connection knows nothing about what this client was following.
        if let Some(thread) = self.inner.watching.get_untracked() {
            self.ask(ClientMsg::Watch { thread });
        }

        // The reader, in this task. A frame this build has no word for is skipped, so a daemon
        // one release ahead still speaks the parts both sides know.
        loop {
            match wire::read::<serde_json::Value>(&mut reading).await {
                Ok(value) => {
                    if let Ok(message) = serde_json::from_value::<ServerMsg>(value) {
                        self.take(message);
                    }
                }
                Err(_) => return,
            }
        }
    }

    /// One message off the wire, into the signals.
    fn take(&self, message: ServerMsg) {
        match message {
            ServerMsg::Welcome { .. } => {}
            ServerMsg::Shells { threads } => self.take_shells(threads),
            ServerMsg::Created { thread } => self.inner.created.set(Some(thread)),
            ServerMsg::Detail { thread, items } => {
                if self.inner.watching.get_untracked() == Some(thread) {
                    self.replace_items(items);
                }
            }
            ServerMsg::Append {
                thread,
                item,
                kind,
                text,
            } => self.append(thread, item, kind, &text),
            ServerMsg::Item { thread, item } => self.upsert(thread, item),
            ServerMsg::Drop { thread, item } => self.drop_row(thread, item),
            ServerMsg::Asks { thread, asks } => {
                if self.inner.watching.get_untracked() == Some(thread) {
                    self.inner.asks.set(asks);
                }
            }
            ServerMsg::Runners { thread, runners } => {
                if self.inner.watching.get_untracked() == Some(thread) {
                    self.inner.runners.set(runners);
                }
            }
            ServerMsg::Plan { thread, markdown } => {
                if self.inner.watching.get_untracked() == Some(thread) {
                    self.inner.plan.set(markdown);
                }
            }
            ServerMsg::Todos { thread, todos } => {
                if self.inner.watching.get_untracked() == Some(thread) {
                    self.inner.todos.set(todos);
                }
            }
            ServerMsg::Catalog { thread, catalog } => {
                if self.inner.watching.get_untracked() == Some(thread) {
                    self.inner.catalog.set(catalog);
                }
            }
            ServerMsg::Refused { reason } => self.inner.problem.set(Some(reason)),
            ServerMsg::Error { message, .. } => self.inner.problem.set(Some(message)),
            ServerMsg::Note { message, .. } => self.inner.note.set(Some(message)),
            ServerMsg::Found { query, rows } => self.inner.found.set(Some((query, rows))),
            ServerMsg::Imports { instance, rows } => {
                self.inner.imports.set(Some((instance, rows)));
            }
            ServerMsg::CommitFiles { root, files } => {
                self.inner.commit_files.set(Some((root, files)));
            }
            ServerMsg::CommitDraft {
                root,
                subject,
                body,
                branch,
            } => {
                self.inner.commit_draft.set(Some(CommitDraft {
                    root,
                    subject,
                    body,
                    branch,
                }));
            }
        }
    }

    /// A fresh thread list, and the news the change carries.
    ///
    /// News is what a person away from a thread would want a toast for: a turn ending, a turn
    /// breaking, a turn stopping to ask. The first list after a connection is not news.
    fn take_shells(&self, threads: Vec<ThreadShell>) {
        let mut news = Vec::new();
        self.inner.threads.with_untracked(|old| {
            if old.is_empty() {
                return;
            }
            for shell in &threads {
                let Some(was) = old.iter().find(|held| held.id == shell.id) else {
                    continue;
                };
                // Done means everything: a turn that ends while runners keep going is not
                // done, and the drain of the last runner is what finishes the story.
                if was.is_working() && !shell.is_working() && shell.state == ThreadState::Idle {
                    news.push(Notice::Done {
                        thread: shell.id,
                        title: shell.title.clone(),
                    });
                }
                if was.state != ThreadState::Failed && shell.state == ThreadState::Failed {
                    news.push(Notice::Failed {
                        thread: shell.id,
                        title: shell.title.clone(),
                        error: shell.last_error.clone().unwrap_or_default(),
                    });
                }
                if shell.asking > was.asking {
                    news.push(Notice::Asking {
                        thread: shell.id,
                        title: shell.title.clone(),
                    });
                }
            }
        });
        // A list that says what the last one said wakes nobody: the daemon says its rows again
        // on every change anywhere, and most of them change nothing here.
        if self.inner.threads.with_untracked(|held| *held != threads) {
            self.inner.threads.set(threads);
        }
        if !self.inner.listed.get_untracked() {
            self.inner.listed.set(true);
        }
        if !news.is_empty() {
            self.inner.news.update(|held| held.extend(news));
        }
    }

    /// Puts one whole row in place, making it when it is new.
    fn upsert(&self, thread: ThreadId, item: TimelineItem) {
        if self.inner.watching.get_untracked() != Some(thread) {
            return;
        }
        let held = self.inner.rows.borrow().get(&item.id).copied();
        match held {
            Some(row) => {
                if row.with_untracked(|held| *held != item) {
                    row.set(item);
                }
            }
            None => {
                let id = item.id;
                self.inner
                    .rows
                    .borrow_mut()
                    .insert(id, RwSignal::new_local(item));
                self.inner.order.update(|order| order.push(id));
            }
        }
    }

    /// Takes one live row away.
    fn drop_row(&self, thread: ThreadId, item: i64) {
        if self.inner.watching.get_untracked() != Some(thread) {
            return;
        }
        self.inner.rows.borrow_mut().remove(&item);
        self.inner
            .order
            .update(|order| order.retain(|id| *id != item));
    }

    /// Grows one streaming row, making it when it is new.
    fn append(&self, thread: ThreadId, item: i64, kind: ItemKind, text: &str) {
        if self.inner.watching.get_untracked() != Some(thread) {
            return;
        }
        let held = self.inner.rows.borrow().get(&item).copied();
        match held {
            Some(row) => row.update(|held| held.text.push_str(text)),
            None => {
                let row = RwSignal::new_local(TimelineItem {
                    id: item,
                    kind,
                    text: text.to_owned(),
                    done: false,
                    ..TimelineItem::default()
                });
                self.inner.rows.borrow_mut().insert(item, row);
                self.inner.order.update(|order| order.push(item));
            }
        }
    }

    /// Puts a whole conversation in place, keeping the signals of rows that stayed.
    ///
    /// Keeping them is what lets a keyed list leave those rows mounted: a snapshot that follows
    /// a settled turn replaces the streaming rows and touches nothing above them.
    fn replace_items(&self, items: Vec<TimelineItem>) {
        let order: Vec<i64> = items.iter().map(|item| item.id).collect();
        {
            let mut rows = self.inner.rows.borrow_mut();
            rows.retain(|id, _| order.contains(id));
            for item in items {
                match rows.get(&item.id) {
                    Some(row) => {
                        if row.with_untracked(|held| *held != item) {
                            row.set(item);
                        }
                    }
                    None => {
                        rows.insert(item.id, RwSignal::new_local(item));
                    }
                }
            }
        }
        if self.inner.order.with_untracked(|held| *held != order) {
            self.inner.order.set(order);
        }
    }
}

/// Asks the running daemon to stop, running turns included.
///
/// For the editor's own exit, when the configuration says the daemon goes with it. Nothing is
/// started: no daemon means nothing to stop.
pub async fn stop_running_daemon() {
    let Some(directory) = zdt_ipc::client::directory() else {
        return;
    };
    let Ok(stream) = tokio::net::UnixStream::connect(directory.join("agentd.sock")).await else {
        return;
    };
    let (mut reading, mut writing) = stream.into_split();
    let hello = ClientMsg::Hello {
        version: zdt_agent::VERSION,
        pid: std::process::id(),
    };
    if wire::write(&mut writing, &hello).await.is_err() {
        return;
    }
    if !matches!(
        wire::read::<ServerMsg>(&mut reading).await,
        Ok(ServerMsg::Welcome { .. })
    ) {
        return;
    }
    let _ = wire::write(&mut writing, &ClientMsg::Shutdown).await;
}

/// Asks a daemon from an older build to stop, in the oldest dialect there was.
///
/// Every version so far takes a matching hello followed by a shutdown, and version 1 is the
/// floor: whatever is running, one of the two hellos this client can speak reaches it.
async fn retire_old_daemon() {
    let Some(directory) = zdt_ipc::client::directory() else {
        return;
    };
    let Ok(stream) = tokio::net::UnixStream::connect(directory.join("agentd.sock")).await else {
        return;
    };
    let (mut reading, mut writing) = stream.into_split();
    let hello = ClientMsg::Hello {
        version: 1,
        pid: std::process::id(),
    };
    if wire::write(&mut writing, &hello).await.is_err() {
        return;
    }
    if !matches!(
        wire::read::<ServerMsg>(&mut reading).await,
        Ok(ServerMsg::Welcome { .. })
    ) {
        return;
    }
    let _ = wire::write(&mut writing, &ClientMsg::Shutdown).await;
}

/// Starts `zdt-agentd`, preferring the binary installed beside this one.
///
/// Its output goes to the agent log directory, because a daemon spawned from a windowed editor
/// has no terminal to speak to. Answers what was started, or what to tell the person: a daemon
/// that cannot be started is a fact for the sidebar, never only a line in a log nobody reads.
fn spawn_daemon() -> Result<String, String> {
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("zdt-agentd")))
        .filter(|path| path.is_file());
    let program = sibling.unwrap_or_else(|| PathBuf::from("zdt-agentd"));

    let log = zdt_core::state::State::discover().map(|state| state.agent().join("logs"));
    let output = log.and_then(|directory| {
        std::fs::create_dir_all(&directory).ok()?;
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(directory.join("daemon.log"))
            .ok()
    });

    let mut command = std::process::Command::new(&program);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null());
    // Its own process group: a Ctrl+C on the terminal that launched the editor reaches the
    // editor's group whole, and the daemon is meant to outlive the editor.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    // The daemon speaks on stderr; a daemon spawned from a windowed editor speaks into the log.
    match output {
        Some(file) => {
            command.stderr(std::process::Stdio::from(file));
        }
        None => {
            command.stderr(std::process::Stdio::null());
        }
    }
    match command.spawn() {
        Ok(_) => Ok(program.display().to_string()),
        Err(error) => {
            tracing::warn!("cannot start {}: {error}", program.display());
            Err(format!(
                "cannot start {}: {error}; build zdt-agentd and put it beside zdt or on the search path",
                program.display()
            ))
        }
    }
}
