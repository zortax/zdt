//! The one loop that touches state.
//!
//! Every command — a client's ask, an adapter's report, a connection coming or going — joins one
//! queue and is handled one at a time. Races like "delete a thread whose turn is settling" are
//! adjudicated here by order of arrival, and nothing else holds the database or the client map.
//!
//! # Live rows
//!
//! Prose streams into rows with negative ids; tool rows are written down the moment they start
//! and move in place. When a stream is cut — a tool begins, or the other stream takes over — the
//! streamed text becomes a finished row and the live id is dropped, which is what keeps the
//! timeline in the order things happened.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use sqlx::SqlitePool;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use zdt_agent::ask::{Ask, Decision};
use zdt_agent::catalog::Catalog;
use zdt_agent::change::{FileStat, TurnDiff};
use zdt_agent::event::{Activity, AgentEvent, StreamKind, WorkItem};
use zdt_agent::mode::RuntimeMode;
use zdt_agent::protocol::{ClientMsg, ServerMsg, WorktreeSpec};
use zdt_agent::thread::{
    DiffStat, ItemKind, ItemStatus, LIVE_ASSISTANT, LIVE_THINKING, ThreadId, ThreadState,
    TimelineItem,
};
use zdt_agent_harness::SessionStart;

use crate::provider::Providers;
use crate::server::ClientId;
use crate::store;

/// What a taken plan is carried out under, when the mode before planning is not known.
const AFTER_PLAN: RuntimeMode = RuntimeMode::AcceptEdits;

/// A worktree the bootstrap task made.
pub struct MadeWorktree {
    /// Its checkout directory.
    path: PathBuf,
    /// The branch checked out in it.
    branch: String,
    /// The branch it started from.
    base: String,
    /// Something worth telling the person, when the bootstrap had to bend.
    note: Option<String>,
}

/// One thing for the engine to do.
pub enum Cmd {
    /// A client finished its handshake.
    Connected {
        /// Which connection.
        id: ClientId,
        /// Where its answers go.
        to_client: UnboundedSender<ServerMsg>,
    },
    /// A client hung up.
    Disconnected {
        /// Which connection.
        id: ClientId,
    },
    /// A client asked something.
    Client {
        /// Which connection.
        id: ClientId,
        /// What it asked.
        message: ClientMsg,
    },
    /// An adapter noticed something.
    Adapter(AgentEvent),
    /// The bootstrap task finished making a worktree, well or badly.
    WorktreeMade {
        /// Which connection asked for it.
        id: ClientId,
        /// The project directory the thread belongs to.
        root: PathBuf,
        /// What the thread is to be called.
        title: String,
        /// Which provider instance drives the thread.
        instance: String,
        /// The worktree, or why there is none.
        made: Result<MadeWorktree, String>,
    },
    /// A git task finished; the answer goes back to whoever asked.
    GitDone {
        /// Which connection asked.
        id: ClientId,
        /// Which thread it worked in.
        thread: ThreadId,
        /// One line about what happened, or what went wrong.
        said: Result<String, String>,
    },
    /// The naming task made up a title.
    Named {
        /// Which thread.
        thread: ThreadId,
        /// What it came up with.
        title: String,
    },
    /// The branch-rename task finished well.
    Rebranched {
        /// Which thread.
        thread: ThreadId,
        /// The branch's fresh name.
        branch: String,
    },
    /// The housekeeping beat: reap idle sessions, settle old threads, prune logs.
    Tick,
    /// The import scan finished; the list goes back to whoever asked.
    ImportsScanned {
        /// Which connection asked.
        id: ClientId,
        /// Which instance was looked under.
        instance: String,
        /// What was found.
        rows: Vec<zdt_agent::protocol::ImportRow>,
    },
    /// The import read finished; the thread is made here, on the loop.
    Imported {
        /// Which connection asked.
        id: ClientId,
        /// Which instance drives the thread.
        instance: String,
        /// The conversation, or nothing when it could not be read.
        dump: Option<zdt_agent_harness::SessionDump>,
    },
    /// The commit scan read the tree; the files go back to whoever asked.
    CommitScanned {
        /// Which connection asked.
        id: ClientId,
        /// Which thread.
        thread: ThreadId,
        /// The files a commit would take.
        files: Vec<FileStat>,
    },
    /// The drafting task wrote a commit message.
    CommitDrafted {
        /// Which connection asked.
        id: ClientId,
        /// Which thread.
        thread: ThreadId,
        /// One imperative line.
        subject: String,
        /// The body under it.
        body: String,
        /// A short branch name for the change.
        branch: String,
    },
}

/// What the configuration tunes about the daemon's housekeeping.
pub struct Tuning {
    /// What a new thread's agent may do unasked.
    pub default_mode: RuntimeMode,
    /// Where worktrees are made: one directory per repository under this one.
    pub worktrees: PathBuf,
    /// Days of quiet before an idle thread settles itself. Zero turns it off.
    pub auto_settle_days: u32,
    /// Minutes of quiet before an idle provider session is stopped. Zero turns it off.
    pub idle_minutes: u64,
    /// Days a raw provider log is kept. Zero turns pruning off.
    pub log_days: u32,
    /// Whether threads name themselves after their first turn.
    pub titles: bool,
    /// The model titles are made with. Empty lets each harness pick its own cheap word.
    pub title_model: String,
    /// The instance commit messages are drafted with. Empty prefers codex, then claude.
    pub commit_instance: String,
    /// The model commit messages are drafted with. Empty lets the harness pick.
    pub commit_model: String,
    /// Where the raw provider logs live.
    pub logs: PathBuf,
}

/// One connected client, as the engine holds it.
struct Connected {
    to_client: UnboundedSender<ServerMsg>,
    /// Which thread it follows, when one.
    watching: Option<ThreadId>,
}

/// What has streamed for a turn that has not settled.
#[derive(Default)]
struct LiveTurn {
    /// The assistant segment still streaming.
    assistant: String,
    /// The thinking segment still streaming.
    thinking: String,
    /// When the streaming thinking segment began, in milliseconds since the epoch.
    thinking_since: Option<u64>,
    /// Which written row each of the provider's tool calls became.
    work: HashMap<String, i64>,
    /// The turn's row in the database, when checkpoints bracket it.
    turn: Option<i64>,
}

/// The daemon's working state.
pub struct Engine {
    pool: SqlitePool,
    /// Every configured provider instance, by name.
    providers: Providers,
    /// What the configuration tunes.
    tuning: Tuning,
    /// The engine's own queue, for tasks that finish off the loop.
    to_self: UnboundedSender<Cmd>,
    clients: HashMap<ClientId, Connected>,
    turns: HashMap<ThreadId, LiveTurn>,
    /// What each thread's turn stopped to ask, oldest first.
    asks: HashMap<ThreadId, Vec<Ask>>,
    /// What each live session says it offers.
    catalogs: HashMap<ThreadId, Catalog>,
    /// What each thread's mode was before it was put in plan mode.
    before_plan: HashMap<ThreadId, RuntimeMode>,
    /// The threads whose catalog a probe was already started for.
    probed: std::collections::HashSet<ThreadId>,
    /// When each thread's session last said or was told anything, for the reaper.
    spoke: HashMap<ThreadId, std::time::Instant>,
    /// What each thread runs beside its main agent right now. Live only: the set dies with the
    /// session that carries it.
    runners: HashMap<ThreadId, Vec<zdt_agent::runner::Runner>>,
    /// Turns that settled while runners still worked. The after checkpoint and the diff wait
    /// for the last runner, so the changes runners make land on the turn that started them.
    parked: HashMap<ThreadId, i64>,
    /// The day the logs were last pruned, in days since the epoch.
    pruned_day: u64,
}

impl Engine {
    /// An engine with no clients.
    pub fn new(
        pool: SqlitePool,
        providers: Providers,
        tuning: Tuning,
        to_self: UnboundedSender<Cmd>,
    ) -> Self {
        Self {
            pool,
            providers,
            tuning,
            to_self,
            clients: HashMap::new(),
            turns: HashMap::new(),
            asks: HashMap::new(),
            catalogs: HashMap::new(),
            before_plan: HashMap::new(),
            probed: std::collections::HashSet::new(),
            spoke: HashMap::new(),
            runners: HashMap::new(),
            parked: HashMap::new(),
            pruned_day: 0,
        }
    }

    /// Handles commands until the queue closes or a shutdown is asked for.
    pub async fn run(mut self, mut inbox: UnboundedReceiver<Cmd>) {
        while let Some(command) = inbox.recv().await {
            match command {
                Cmd::Connected { id, to_client } => {
                    let shells = self.shells().await;
                    let _ = to_client.send(shells);
                    self.clients.insert(
                        id,
                        Connected {
                            to_client,
                            watching: None,
                        },
                    );
                }
                Cmd::Disconnected { id } => {
                    self.clients.remove(&id);
                }
                Cmd::Client { id, message } => {
                    if let ClientMsg::Shutdown = message {
                        tracing::info!("shutting down");
                        self.providers.stop_all().await;
                        return;
                    }
                    self.client(id, message).await;
                }
                Cmd::Adapter(event) => self.adapter_event(event).await,
                Cmd::WorktreeMade {
                    id,
                    root,
                    title,
                    instance,
                    made,
                } => self.worktree_made(id, &root, &title, &instance, made).await,
                Cmd::GitDone { id, thread, said } => match said {
                    Ok(message) => {
                        self.to(id, ServerMsg::Note { thread, message });
                        self.broadcast_shells().await;
                    }
                    Err(message) => self.error(id, Some(thread), &message),
                },
                Cmd::Named { thread, title } => self.named(thread, title).await,
                Cmd::CommitScanned { id, thread, files } => {
                    self.to(id, ServerMsg::CommitFiles { thread, files });
                }
                Cmd::CommitDrafted {
                    id,
                    thread,
                    subject,
                    body,
                    branch,
                } => {
                    self.to(
                        id,
                        ServerMsg::CommitDraft {
                            thread,
                            subject,
                            body,
                            branch,
                        },
                    );
                }
                Cmd::Rebranched { thread, branch } => {
                    let _ = store::set_branch(&self.pool, thread, &branch).await;
                    self.broadcast_shells().await;
                }
                Cmd::Tick => self.tick().await,
                Cmd::ImportsScanned { id, instance, rows } => {
                    self.to(id, ServerMsg::Imports { instance, rows });
                }
                Cmd::Imported { id, instance, dump } => self.imported(id, &instance, dump).await,
            }
        }
    }

    /// One client's ask.
    async fn client(&mut self, id: ClientId, message: ClientMsg) {
        match message {
            ClientMsg::Hello { .. } => self.refuse(id, "already said hello"),
            ClientMsg::Create {
                root,
                title,
                worktree,
                instance,
            } => self.create(id, &root, &title, worktree, instance).await,
            ClientMsg::Revert { thread, turn } => self.revert(id, thread, turn).await,
            ClientMsg::Commit {
                thread,
                message,
                push,
                branch,
                paths,
            } => self.commit(id, thread, message, push, branch, paths).await,
            ClientMsg::DraftCommit { thread } => self.draft_commit(id, thread).await,
            ClientMsg::Send { thread, text } => self.send(id, thread, text).await,
            ClientMsg::Interrupt { thread } => {
                let interrupted = match self.provider_of(thread).await {
                    Some(provider) => provider.interrupt(thread).await,
                    None => return self.refuse(id, "that thread's provider is not configured"),
                };
                if let Err(error) = interrupted {
                    self.error(id, Some(thread), &error.to_string());
                }
            }
            ClientMsg::Decide {
                thread,
                id: ask,
                decision,
            } => self.decide(id, thread, &ask, decision).await,
            ClientMsg::Answer {
                thread,
                id: ask,
                answers,
            } => self.answer(id, thread, &ask, answers).await,
            ClientMsg::Implement { thread } => self.implement(id, thread).await,
            ClientMsg::SetMode { thread, mode } => self.set_mode(id, thread, mode).await,
            ClientMsg::SetModel { thread, model } => {
                let _ = store::set_model(&self.pool, thread, &model).await;
                if let Some(provider) = self.provider_of(thread).await {
                    let _ = provider.set_model(thread, model).await;
                }
                self.broadcast_shells().await;
            }
            ClientMsg::SetEffort { thread, effort } => {
                // Refused while a turn runs: an adapter whose effort rides only a spawn would
                // respawn under the running turn and lose it.
                let busy = store::thread_row(&self.pool, thread)
                    .await
                    .ok()
                    .flatten()
                    .is_some_and(|row| row.state.is_busy());
                if busy {
                    self.refuse(id, "a turn is running; wait for it first");
                    return;
                }
                let _ = store::set_effort(&self.pool, thread, &effort).await;
                self.broadcast_shells().await;
            }
            ClientMsg::Watch { thread } => {
                if let Some(client) = self.clients.get_mut(&id) {
                    client.watching = Some(thread);
                }
                // Looking at a thread reads it, and takes the woke pill off a snooze that has
                // already ended.
                if store::clear_attention(&self.pool, thread)
                    .await
                    .unwrap_or(false)
                {
                    self.broadcast_shells().await;
                }
                let detail = self.detail(thread).await;
                self.to(id, detail);
                let asks = self.asks.get(&thread).cloned().unwrap_or_default();
                self.to(id, ServerMsg::Asks { thread, asks });
                let runners = self.runners.get(&thread).cloned().unwrap_or_default();
                self.to(id, ServerMsg::Runners { thread, runners });
                let row = store::thread_row(&self.pool, thread).await.ok().flatten();
                // A thread nobody has spoken through yet still shows what it offers: a short
                // probe session answers with the commands and the models.
                if let Some(row) = &row
                    && self.catalogs.get(&thread).is_none_or(Catalog::is_empty)
                    && self.probed.insert(thread)
                    && let Some(provider) = self.providers.get(&row.instance)
                {
                    provider.probe(thread, row.root.clone());
                }
                self.to(
                    id,
                    ServerMsg::Plan {
                        thread,
                        markdown: row.and_then(|row| row.proposed_plan),
                    },
                );
                let todos = store::todos(&self.pool, thread).await.unwrap_or_default();
                self.to(id, ServerMsg::Todos { thread, todos });
                let catalog = self.catalogs.get(&thread).cloned().unwrap_or_default();
                self.to(id, ServerMsg::Catalog { thread, catalog });
            }
            ClientMsg::Unwatch => {
                if let Some(client) = self.clients.get_mut(&id) {
                    client.watching = None;
                }
            }
            ClientMsg::Delete { thread } => self.delete(id, thread).await,
            ClientMsg::Pin { thread, order } => {
                let _ = store::set_pinned(&self.pool, thread, order).await;
                self.broadcast_shells().await;
            }
            ClientMsg::Snooze { thread, until_ms } => {
                let _ = store::set_snoozed(&self.pool, thread, until_ms).await;
                self.broadcast_shells().await;
            }
            ClientMsg::Settle { thread, settled } => self.settle_cmd(id, thread, settled).await,
            ClientMsg::Archive { thread, archived } => {
                let _ = store::set_archived(&self.pool, thread, archived).await;
                self.broadcast_shells().await;
            }
            ClientMsg::MarkUnread { thread, unread } => {
                let _ = store::set_unread(&self.pool, thread, unread).await;
                self.broadcast_shells().await;
            }
            ClientMsg::Rename { thread, title } => self.rename(id, thread, title).await,
            ClientMsg::SetDraft { thread, text } => {
                let _ = store::set_draft(&self.pool, thread, &text).await;
                self.broadcast_shells().await;
            }
            ClientMsg::Search { query } => self.search(id, query).await,
            ClientMsg::ListImports { instance } => self.list_imports(id, instance).await,
            ClientMsg::Import {
                instance,
                id: session,
            } => self.import(id, instance, session).await,
            ClientMsg::Shutdown => unreachable!("handled by the loop"),
        }
    }

    /// Puts a thread away as done, or takes it back out.
    ///
    /// Settling is refused while the thread is active or owes somebody an answer: putting away
    /// running work is how work gets lost. Taking it back out is never refused.
    async fn settle_cmd(&mut self, id: ClientId, thread: ThreadId, settled: bool) {
        if settled {
            let row = store::thread_row(&self.pool, thread).await.ok().flatten();
            let Some(row) = row else {
                self.refuse(id, &format!("thread {thread} is not there"));
                return;
            };
            if row.state.is_busy() {
                self.refuse(id, "a turn is running; wait for it first");
                return;
            }
            if self.asks.get(&thread).is_some_and(|open| !open.is_empty()) {
                self.refuse(id, "the thread is asking something; answer it first");
                return;
            }
            // Settled means read.
            let _ = store::set_unread(&self.pool, thread, false).await;
        }
        let _ = store::set_settled(&self.pool, thread, settled).await;
        self.broadcast_shells().await;
    }

    /// Calls a thread something else. An empty title asks for a generated one.
    async fn rename(&mut self, id: ClientId, thread: ThreadId, title: String) {
        let title = title.trim().to_owned();
        if title.is_empty() {
            let Ok(Some(row)) = store::thread_row(&self.pool, thread).await else {
                self.refuse(id, &format!("thread {thread} is not there"));
                return;
            };
            // Back to the placeholder, so the naming task's answer is taken when it lands.
            let _ = store::set_title(&self.pool, thread, "New thread").await;
            self.broadcast_shells().await;
            self.generate_title(thread, &row.instance);
            return;
        }
        let _ = store::set_title(&self.pool, thread, &title).await;
        self.broadcast_shells().await;
    }

    /// Answers which threads' conversations contain the words.
    async fn search(&mut self, id: ClientId, query: String) {
        let query = query.trim().to_owned();
        if query.is_empty() {
            self.to(
                id,
                ServerMsg::Found {
                    query,
                    rows: Vec::new(),
                },
            );
            return;
        }
        let rows = store::search_messages(&self.pool, &query, 20)
            .await
            .unwrap_or_default();
        self.to(id, ServerMsg::Found { query, rows });
    }

    /// Lists the conversations `instance`'s provider holds on disk, off the loop.
    ///
    /// Conversations a thread already resumes are left out: they are here, not importable.
    async fn list_imports(&mut self, id: ClientId, instance: String) {
        let Some(provider) = self.providers.get(&instance).cloned() else {
            self.refuse(id, &format!("no provider instance is called {instance}"));
            return;
        };
        let held = store::resumes(&self.pool).await.unwrap_or_default();
        let to_self = self.to_self.clone();
        tokio::task::spawn_blocking(move || {
            let rows = provider
                .importable()
                .into_iter()
                .filter(|found| !held.contains(&found.id))
                .map(|found| zdt_agent::protocol::ImportRow {
                    id: found.id,
                    title: found.title,
                    root: found.cwd,
                    at_ms: found.at_ms,
                })
                .collect();
            let _ = to_self.send(Cmd::ImportsScanned { id, instance, rows });
        });
    }

    /// Reads one provider-side conversation off the loop; the thread is made when it comes back.
    async fn import(&mut self, id: ClientId, instance: String, session: String) {
        let Some(provider) = self.providers.get(&instance).cloned() else {
            self.refuse(id, &format!("no provider instance is called {instance}"));
            return;
        };
        let to_self = self.to_self.clone();
        tokio::task::spawn_blocking(move || {
            let dump = provider.import_dump(&session);
            let _ = to_self.send(Cmd::Imported { id, instance, dump });
        });
    }

    /// The read conversation, made into a thread: project, rows, resume cursor, title.
    async fn imported(
        &mut self,
        id: ClientId,
        instance: &str,
        dump: Option<zdt_agent_harness::SessionDump>,
    ) {
        let Some(dump) = dump else {
            self.refuse(id, "that conversation could not be read");
            return;
        };
        let Ok(root) = std::fs::canonicalize(&dump.cwd) else {
            self.refuse(
                id,
                &format!("{} is not a directory any more", dump.cwd.display()),
            );
            return;
        };
        let name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.to_string_lossy().into_owned());
        let provider_word = self
            .providers
            .get(instance)
            .map_or("", crate::provider::Provider::kind);
        let made = async {
            let project = store::ensure_project(&self.pool, &root.to_string_lossy(), &name).await?;
            let thread = store::create_thread(
                &self.pool,
                project,
                &dump.title,
                self.tuning.default_mode,
                &store::WorktreeCols::default(),
                instance,
                provider_word,
            )
            .await?;
            store::set_resume(&self.pool, thread, Some(&dump.id)).await?;
            for line in &dump.lines {
                let item = TimelineItem {
                    kind: if line.user {
                        ItemKind::User
                    } else {
                        ItemKind::Assistant
                    },
                    text: line.text.clone(),
                    done: true,
                    ..TimelineItem::default()
                };
                store::add_item(&self.pool, thread, &item).await?;
            }
            anyhow::Ok(thread)
        }
        .await;

        match made {
            Ok(thread) => {
                self.to(id, ServerMsg::Created { thread });
                self.broadcast_shells().await;
            }
            Err(error) => self.error(id, None, &error.to_string()),
        }
    }

    /// Makes a thread for `root`, in a worktree of its own when one is asked for.
    ///
    /// The worktree's git work runs off the loop: a fetch can take seconds, and nothing else
    /// must wait on it. The thread row is written when [`Cmd::WorktreeMade`] comes back.
    async fn create(
        &mut self,
        id: ClientId,
        root: &Path,
        title: &str,
        worktree: Option<WorktreeSpec>,
        instance: String,
    ) {
        let Ok(real) = std::fs::canonicalize(root) else {
            self.refuse(id, &format!("{} is not a directory", root.display()));
            return;
        };
        let instance = if instance.is_empty() {
            self.providers.default_name().to_owned()
        } else {
            instance
        };
        if self.providers.get(&instance).is_none() {
            self.refuse(id, &format!("no provider instance is called {instance}"));
            return;
        }
        let title = if title.is_empty() {
            "New thread"
        } else {
            title
        };

        if let Some(spec) = worktree {
            let parent = self.tuning.worktrees.clone();
            let to_self = self.to_self.clone();
            let (root, title) = (real, title.to_owned());
            tokio::task::spawn_blocking(move || {
                let made = bootstrap_worktree(&root, &parent, &spec);
                let _ = to_self.send(Cmd::WorktreeMade {
                    id,
                    root,
                    title,
                    instance,
                    made,
                });
            });
            return;
        }

        self.write_thread(
            id,
            &real,
            title,
            &store::WorktreeCols::default(),
            &instance,
            None,
        )
        .await;
    }

    /// The bootstrap task's answer: the thread row, or the reason there is none.
    async fn worktree_made(
        &mut self,
        id: ClientId,
        root: &Path,
        title: &str,
        instance: &str,
        made: Result<MadeWorktree, String>,
    ) {
        match made {
            Ok(made) => {
                let columns = store::WorktreeCols {
                    path: made.path.to_string_lossy().into_owned(),
                    branch: made.branch,
                    base: made.base,
                };
                self.write_thread(id, root, title, &columns, instance, made.note)
                    .await;
            }
            Err(error) => self.error(id, None, &error),
        }
    }

    /// Writes the thread row down and tells everyone.
    async fn write_thread(
        &mut self,
        id: ClientId,
        root: &Path,
        title: &str,
        worktree: &store::WorktreeCols,
        instance: &str,
        note: Option<String>,
    ) {
        let name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.to_string_lossy().into_owned());
        let mode = self.tuning.default_mode;
        let provider = self
            .providers
            .get(instance)
            .map_or("", crate::provider::Provider::kind);
        let made = async {
            let project = store::ensure_project(&self.pool, &root.to_string_lossy(), &name).await?;
            store::create_thread(
                &self.pool, project, title, mode, worktree, instance, provider,
            )
            .await
        }
        .await;

        match made {
            Ok(thread) => {
                self.to(id, ServerMsg::Created { thread });
                if let Some(message) = note {
                    self.to(id, ServerMsg::Note { thread, message });
                }
                self.broadcast_shells().await;
            }
            Err(error) => self.error(id, None, &error.to_string()),
        }
    }

    /// Sends a prompt into a thread. A busy thread takes it as steering into the running turn.
    async fn send(&mut self, id: ClientId, thread: ThreadId, text: String) {
        let row = match store::thread_row(&self.pool, thread).await {
            Ok(Some(row)) => row,
            Ok(None) => {
                self.refuse(id, &format!("thread {thread} is not there"));
                return;
            }
            Err(error) => {
                self.error(id, Some(thread), &error.to_string());
                return;
            }
        };

        // A message answers a proposed plan by refining it; the proposal comes back or not.
        if row.proposed_plan.is_some() {
            let _ = store::set_plan(&self.pool, thread, None).await;
            self.to_watchers(
                thread,
                ServerMsg::Plan {
                    thread,
                    markdown: None,
                },
            );
        }

        // A prompt is real activity: it wakes the thread, takes it back out of the shelves, and
        // uses up the draft it came from.
        let _ = store::set_settled(&self.pool, thread, false).await;
        let _ = store::set_archived(&self.pool, thread, false).await;
        let _ = store::set_snoozed(&self.pool, thread, 0).await;
        let _ = store::set_draft(&self.pool, thread, "").await;
        self.spoke.insert(thread, std::time::Instant::now());

        let steering = row.state.is_busy();
        // A turn still parked behind runners closes before the new prompt's row, so its changes
        // card sits with the turn that made them.
        if !steering && let Some(parked) = self.parked.remove(&thread) {
            self.close_turn(thread, parked).await;
            self.refresh_watchers(thread).await;
        }
        let first_item = self.write_user_row(thread, &text).await;
        if !steering {
            let _ = store::set_state(&self.pool, thread, ThreadState::Starting, None).await;
            self.turns.insert(thread, LiveTurn::default());
            // The before checkpoint. Awaited here on purpose: the capture must land before the
            // agent's first edit, and it is local git work measured in milliseconds.
            if let Some(first_item) = first_item {
                let turn = self
                    .open_turn(thread, first_item, row.resume.as_deref(), &row.root)
                    .await;
                if let Some(live) = self.turns.get_mut(&thread) {
                    live.turn = turn;
                }
            }
        }
        self.broadcast_shells().await;

        let Some(provider) = self.providers.get(&row.instance) else {
            self.refuse(id, "that thread's provider is not configured");
            return;
        };
        let start = SessionStart {
            thread,
            cwd: row.root,
            resume: row.resume,
            model: if row.model.is_empty() {
                self.providers.default_model(&row.instance).to_owned()
            } else {
                row.model
            },
            effort: row.effort,
            mode: row.mode,
        };
        if let Err(error) = provider.send_turn(start, text).await {
            let said = error.to_string();
            let _ = store::set_state(&self.pool, thread, ThreadState::Failed, Some(&said)).await;
            self.turns.remove(&thread);
            self.broadcast_shells().await;
            self.error(id, Some(thread), &said);
        }
    }

    /// Writes one user row down and shows it to watchers, answering its id.
    async fn write_user_row(&mut self, thread: ThreadId, text: &str) -> Option<i64> {
        let mut item = TimelineItem {
            kind: ItemKind::User,
            text: text.to_owned(),
            done: true,
            ..TimelineItem::default()
        };
        let id = store::add_item(&self.pool, thread, &item).await.ok()?;
        item.id = id;
        item.at_ms = zdt_core::state::now_ms();
        self.to_watchers(thread, ServerMsg::Item { thread, item });
        Some(id)
    }

    /// Opens a turn: its row, and the checkpoint captured before anything runs.
    ///
    /// Nothing when the thread's directory is not a repository — such a turn has no diff and no
    /// revert, and everything else still works.
    async fn open_turn(
        &mut self,
        thread: ThreadId,
        first_item: i64,
        resume: Option<&str>,
        root: &Path,
    ) -> Option<i64> {
        let turn = store::add_turn(&self.pool, thread, first_item, resume, "")
            .await
            .ok()?;
        let reference = zdt_git::checkpoint::turn_ref(thread.0, turn, "before");
        let captured = capture_checkpoint(root.to_path_buf(), reference.clone()).await;
        if captured.is_none() {
            return Some(turn);
        }
        let _ = store::set_turn_before(&self.pool, turn, &reference).await;
        Some(turn)
    }

    /// Decides an open tool ask.
    async fn decide(&mut self, id: ClientId, thread: ThreadId, ask: &str, decision: Decision) {
        if !self.take_ask(thread, ask) {
            self.refuse(id, "that ask is no longer open");
            return;
        }
        if let Some(provider) = self.provider_of(thread).await
            && let Err(error) = provider.decide(thread, ask.to_owned(), decision).await
        {
            self.error(id, Some(thread), &error.to_string());
        }
        self.push_asks(thread).await;
    }

    /// Answers an open question ask.
    async fn answer(
        &mut self,
        id: ClientId,
        thread: ThreadId,
        ask: &str,
        answers: Vec<Vec<String>>,
    ) {
        if !self.take_ask(thread, ask) {
            self.refuse(id, "that ask is no longer open");
            return;
        }
        if let Some(provider) = self.provider_of(thread).await
            && let Err(error) = provider.answer(thread, ask.to_owned(), answers).await
        {
            self.error(id, Some(thread), &error.to_string());
        }
        self.push_asks(thread).await;
    }

    /// Removes one ask from a thread's open list. `false` when it was not there.
    fn take_ask(&mut self, thread: ThreadId, ask: &str) -> bool {
        let Some(open) = self.asks.get_mut(&thread) else {
            return false;
        };
        let before = open.len();
        open.retain(|held| held.id != ask);
        before != open.len()
    }

    /// Takes the proposed plan and starts the turn that carries it out.
    async fn implement(&mut self, id: ClientId, thread: ThreadId) {
        let row = match store::thread_row(&self.pool, thread).await {
            Ok(Some(row)) => row,
            _ => {
                self.refuse(id, &format!("thread {thread} is not there"));
                return;
            }
        };
        if row.proposed_plan.is_none() {
            self.refuse(id, "there is no proposed plan to take");
            return;
        }
        if row.state.is_busy() {
            self.refuse(id, "a turn is running; wait for it first");
            return;
        }
        let _ = store::set_plan(&self.pool, thread, None).await;
        self.to_watchers(
            thread,
            ServerMsg::Plan {
                thread,
                markdown: None,
            },
        );

        // Out of plan mode, back to what the thread ran as before it planned.
        let mode = self.before_plan.remove(&thread).unwrap_or(AFTER_PLAN);
        let _ = store::set_mode(&self.pool, thread, mode).await;
        if let Some(provider) = self.providers.get(&row.instance) {
            let _ = provider.set_mode(thread, mode).await;
        }

        self.send(id, thread, "The plan is approved. Carry it out.".to_owned())
            .await;
    }

    /// Moves a thread to `mode`.
    async fn set_mode(&mut self, _id: ClientId, thread: ThreadId, mode: RuntimeMode) {
        if mode == RuntimeMode::Plan {
            let held = store::thread_row(&self.pool, thread)
                .await
                .ok()
                .flatten()
                .map(|row| row.mode)
                .filter(|held| *held != RuntimeMode::Plan);
            if let Some(held) = held {
                self.before_plan.insert(thread, held);
            }
        }
        let _ = store::set_mode(&self.pool, thread, mode).await;
        if let Some(provider) = self.provider_of(thread).await {
            let _ = provider.set_mode(thread, mode).await;
        }
        self.broadcast_shells().await;
    }

    /// Puts the working tree back to before `turn` ran, and forgets that turn onward.
    ///
    /// The conversation goes back with it: rows from the turn's prompt onward are dropped, and
    /// the provider resumes from the cursor the turn started with.
    async fn revert(&mut self, id: ClientId, thread: ThreadId, turn: i64) {
        let row = match store::thread_row(&self.pool, thread).await {
            Ok(Some(row)) => row,
            _ => {
                self.refuse(id, &format!("thread {thread} is not there"));
                return;
            }
        };
        if row.state.is_busy() {
            self.refuse(id, "a turn is running; interrupt it first");
            return;
        }
        let opened = match store::turn_row(&self.pool, turn).await {
            Ok(Some(opened)) if opened.thread == thread => opened,
            _ => {
                self.refuse(id, "that turn is not there any more");
                return;
            }
        };
        if opened.before_ref.is_empty() {
            self.refuse(id, "that turn has no checkpoint to go back to");
            return;
        }

        // The session that carried the dropped turns is stale; the stored cursor resumes from
        // before them.
        if let Some(provider) = self.providers.get(&row.instance) {
            provider.stop(thread).await;
        }

        let restored = {
            let (root, reference) = (row.root.clone(), opened.before_ref.clone());
            tokio::task::spawn_blocking(move || {
                let repo = zdt_git::Repo::open(&root).map_err(|error| error.to_string())?;
                zdt_git::checkpoint::restore(&repo, &reference).map_err(|error| error.to_string())
            })
            .await
        };
        match restored {
            Ok(Ok(())) => {}
            Ok(Err(said)) => {
                self.error(id, Some(thread), &said);
                return;
            }
            Err(join) => {
                self.error(id, Some(thread), &join.to_string());
                return;
            }
        }

        // The dropped turns' checkpoints go with them.
        if let Ok(dropped) = store::turns_from(&self.pool, thread, turn).await {
            let root = row.root.clone();
            tokio::task::spawn_blocking(move || {
                let Ok(repo) = zdt_git::Repo::open(&root) else {
                    return;
                };
                for gone in dropped {
                    let prefix = format!("refs/zdt/checkpoints/{}/{gone}/", thread.0);
                    let _ = zdt_git::checkpoint::forget(&repo, &prefix);
                }
            });
        }

        let _ = store::delete_items_from(&self.pool, thread, opened.first_item).await;
        let _ = store::delete_turns_from(&self.pool, thread, turn).await;
        let _ = store::set_resume(&self.pool, thread, opened.resume_before.as_deref()).await;
        self.refresh_diff_stat(thread, &row.root).await;

        self.to(
            id,
            ServerMsg::Note {
                thread,
                message: "went back to before the turn".to_owned(),
            },
        );
        self.broadcast_shells().await;
        self.refresh_watchers(thread).await;
    }

    /// Commits the thread's whole working tree, and pushes it when asked.
    ///
    /// A non-empty `branch` is made at `HEAD` first and the commit lands on it. The git work
    /// runs off the loop — a push crosses the network — and the answer comes back as
    /// [`Cmd::GitDone`].
    async fn commit(
        &mut self,
        id: ClientId,
        thread: ThreadId,
        message: String,
        push: bool,
        branch: String,
        paths: Vec<String>,
    ) {
        let row = match store::thread_row(&self.pool, thread).await {
            Ok(Some(row)) => row,
            _ => {
                self.refuse(id, &format!("thread {thread} is not there"));
                return;
            }
        };
        if row.state.is_busy() {
            self.refuse(id, "a turn is running; wait for it first");
            return;
        }
        let to_self = self.to_self.clone();
        let root = row.root;
        // A worktree thread follows its checkout onto the new branch.
        let tracks = !row.branch.is_empty();
        tokio::task::spawn_blocking(move || {
            let said = (|| {
                let repo = zdt_git::Repo::open(&root).map_err(|error| error.to_string())?;
                if !branch.is_empty() {
                    zdt_git::commit::switch_new(&repo, &branch)
                        .map_err(|error| error.to_string())?;
                }
                let made = if paths.is_empty() {
                    zdt_git::commit::commit_all(&repo, &message)
                } else {
                    zdt_git::commit::commit_paths(&repo, &message, &paths)
                }
                .map_err(|error| error.to_string())?;
                let short: String = made.chars().take(7).collect();
                let mut told = match (branch.is_empty(), push) {
                    (true, false) => format!("committed {short}"),
                    (true, true) => format!("committed {short} and pushed"),
                    (false, _) => format!("committed {short} on {branch}"),
                };
                if push {
                    zdt_git::commit::push(&repo).map_err(|error| error.to_string())?;
                    if !branch.is_empty() {
                        told.push_str(" and pushed");
                    }
                }
                Ok(told)
            })();
            if said.is_ok() && !branch.is_empty() && tracks {
                let _ = to_self.send(Cmd::Rebranched {
                    thread,
                    branch: branch.clone(),
                });
            }
            let _ = to_self.send(Cmd::GitDone { id, thread, said });
        });
    }

    /// Scans what a commit would take and has a message drafted, both off the loop.
    ///
    /// The files go back the moment the tree is read; the draft follows when the model answers.
    /// A person types over either without waiting.
    async fn draft_commit(&mut self, id: ClientId, thread: ThreadId) {
        let row = match store::thread_row(&self.pool, thread).await {
            Ok(Some(row)) => row,
            _ => {
                self.refuse(id, &format!("thread {thread} is not there"));
                return;
            }
        };
        let Some(provider) = self
            .providers
            .messenger(&self.tuning.commit_instance)
            .cloned()
        else {
            self.refuse(id, "no provider instance can draft a message");
            return;
        };
        let model = self.tuning.commit_model.clone();
        let to_self = self.to_self.clone();
        let root = row.root;
        let on_branch = head_branch(&root);
        tokio::spawn(async move {
            let scanned = tokio::task::spawn_blocking(move || {
                let repo = zdt_git::Repo::open(&root).ok()?;
                zdt_git::commit::pending(&repo).ok()
            })
            .await
            .ok()
            .flatten()
            .unwrap_or_default();

            let files: Vec<FileStat> = scanned
                .files
                .iter()
                .map(|file| FileStat {
                    path: file.path.clone(),
                    added: file.added,
                    removed: file.removed,
                    binary: file.binary,
                })
                .collect();
            let _ = to_self.send(Cmd::CommitScanned {
                id,
                thread,
                files: files.clone(),
            });
            if files.is_empty() {
                return;
            }

            let prompt = commit_prompt(&on_branch, &files, &scanned.patch);
            let Some(said) = provider.generate(&model, &prompt).await else {
                return;
            };
            let Some((subject, body, branch)) = parse_commit_draft(&said) else {
                return;
            };
            let _ = to_self.send(Cmd::CommitDrafted {
                id,
                thread,
                subject,
                body,
                branch,
            });
        });
    }

    /// Takes a thread away. A worktree thread's worktree and branch go with it, and so do its
    /// checkpoints.
    async fn delete(&mut self, id: ClientId, thread: ThreadId) {
        if let Some(provider) = self.provider_of(thread).await {
            provider.stop(thread).await;
        }
        self.turns.remove(&thread);
        self.asks.remove(&thread);
        self.catalogs.remove(&thread);
        self.before_plan.remove(&thread);
        self.spoke.remove(&thread);
        self.runners.remove(&thread);
        self.parked.remove(&thread);
        let row = store::thread_row(&self.pool, thread).await.ok().flatten();
        if let Err(error) = store::delete_thread(&self.pool, thread).await {
            self.error(id, Some(thread), &error.to_string());
            return;
        }
        if let Some(row) = row {
            tokio::task::spawn_blocking(move || {
                let Ok(repo) = zdt_git::Repo::open(&row.project_root) else {
                    return;
                };
                let prefix = zdt_git::checkpoint::thread_prefix(thread.0);
                if let Err(error) = zdt_git::checkpoint::forget(&repo, &prefix) {
                    tracing::warn!("thread {thread}: checkpoints not forgotten: {error}");
                }
                if let Some(worktree) = row.worktree {
                    let branch = (!row.branch.is_empty()).then_some(row.branch.as_str());
                    if let Err(error) = zdt_git::worktree::remove(&repo, &worktree, branch) {
                        tracing::warn!("thread {thread}: worktree not removed: {error}");
                    }
                }
            });
        }
        for client in self.clients.values_mut() {
            if client.watching == Some(thread) {
                client.watching = None;
            }
        }
        self.broadcast_shells().await;
    }

    /// One adapter report.
    async fn adapter_event(&mut self, event: AgentEvent) {
        let thread = event.thread();
        self.spoke.insert(thread, std::time::Instant::now());
        match event {
            AgentEvent::SessionStarted { session, .. } => {
                let _ = store::set_resume(&self.pool, thread, Some(&session)).await;
            }
            AgentEvent::Catalog { catalog, .. } => {
                let held = self.catalogs.entry(thread).or_default();
                held.merge(catalog);
                let catalog = held.clone();
                self.to_watchers(thread, ServerMsg::Catalog { thread, catalog });
            }
            AgentEvent::State { activity, .. } => match activity {
                Activity::Running => {
                    let _ = store::set_state(&self.pool, thread, ThreadState::Working, None).await;
                    self.broadcast_shells().await;
                }
                // Runners and open asks live inside the session's process; a stopped session
                // takes them along.
                Activity::Stopped => {
                    self.set_runners(thread, Vec::new()).await;
                    self.clear_asks(thread).await;
                }
                // Idle follows a turn's end; the turn's own event has already said what the
                // thread's state is.
                Activity::Starting | Activity::Idle => {}
            },
            AgentEvent::Delta { kind, text, .. } => self.delta(thread, kind, text).await,
            AgentEvent::Work { item, .. } => self.work(thread, item).await,
            AgentEvent::Runners { runners, .. } => self.set_runners(thread, runners).await,
            AgentEvent::Asked { ask, .. } => {
                // A question must surface; a snoozed thread wakes for it.
                let _ = store::set_snoozed(&self.pool, thread, 0).await;
                self.asks.entry(thread).or_default().push(ask);
                self.push_asks(thread).await;
            }
            AgentEvent::AskGone { id, .. } => {
                if self.take_ask(thread, &id) {
                    self.push_asks(thread).await;
                }
            }
            AgentEvent::PlanProposed { markdown, .. } => {
                let _ = store::set_plan(&self.pool, thread, Some(&markdown)).await;
                self.to_watchers(
                    thread,
                    ServerMsg::Plan {
                        thread,
                        markdown: Some(markdown),
                    },
                );
                self.broadcast_shells().await;
            }
            AgentEvent::Todos { todos, .. } => {
                let _ = store::set_todos(&self.pool, thread, &todos).await;
                self.to_watchers(thread, ServerMsg::Todos { thread, todos });
            }
            AgentEvent::Usage {
                context_tokens,
                context_limit,
                ..
            } => {
                let _ = store::set_context(&self.pool, thread, context_tokens, context_limit).await;
            }
            AgentEvent::TurnDone {
                error, cost_usd, ..
            } => {
                if let Some(cost) = cost_usd {
                    let _ = store::add_cost(&self.pool, thread, cost).await;
                }
                self.settle(thread, error).await;
            }
            AgentEvent::Noted { message, .. } => {
                self.to_watchers(thread, ServerMsg::Note { thread, message });
            }
            AgentEvent::Fatal { error, .. } => {
                self.set_runners(thread, Vec::new()).await;
                self.clear_asks(thread).await;
                self.settle(thread, Some(error)).await;
            }
        }
    }

    /// One piece of streamed prose. The other stream, if it was live, is finished first.
    async fn delta(&mut self, thread: ThreadId, kind: StreamKind, text: String) {
        match kind {
            StreamKind::Assistant => self.cut_thinking(thread).await,
            StreamKind::Thinking => self.cut_assistant(thread).await,
        }
        let turn = self.turns.entry(thread).or_default();
        let (item, kind_out) = match kind {
            StreamKind::Assistant => {
                turn.assistant.push_str(&text);
                (LIVE_ASSISTANT, ItemKind::Assistant)
            }
            StreamKind::Thinking => {
                if turn.thinking_since.is_none() {
                    turn.thinking_since = Some(zdt_core::state::now_ms());
                }
                turn.thinking.push_str(&text);
                (LIVE_THINKING, ItemKind::Thinking)
            }
        };
        self.to_watchers(
            thread,
            ServerMsg::Append {
                thread,
                item,
                kind: kind_out,
                text,
            },
        );
    }

    /// One tool or task moving. New keys become rows; known keys move theirs.
    async fn work(&mut self, thread: ThreadId, work: WorkItem) {
        self.cut_assistant(thread).await;
        self.cut_thinking(thread).await;
        let turn = self.turns.entry(thread).or_default();
        let known = turn.work.get(&work.key).copied();
        let mut item = TimelineItem {
            id: known.unwrap_or_default(),
            kind: work.kind,
            text: work.summary,
            name: work.name,
            tool: work.tool,
            status: work.status,
            detail: work.detail,
            done: work.status != ItemStatus::Running,
            at_ms: zdt_core::state::now_ms(),
            elapsed_ms: 0,
        };
        match known {
            Some(id) => {
                let _ =
                    store::update_item(&self.pool, id, &item.text, item.status, &item.detail).await;
            }
            None => {
                let Ok(id) = store::add_item(&self.pool, thread, &item).await else {
                    return;
                };
                item.id = id;
                if let Some(turn) = self.turns.get_mut(&thread) {
                    turn.work.insert(work.key, id);
                }
            }
        }
        self.to_watchers(thread, ServerMsg::Item { thread, item });
    }

    /// Writes the streamed assistant segment down as a finished row, when there is one.
    async fn cut_assistant(&mut self, thread: ThreadId) {
        let Some(turn) = self.turns.get_mut(&thread) else {
            return;
        };
        if turn.assistant.is_empty() {
            return;
        }
        let mut item = TimelineItem {
            kind: ItemKind::Assistant,
            text: std::mem::take(&mut turn.assistant),
            done: true,
            at_ms: zdt_core::state::now_ms(),
            ..TimelineItem::default()
        };
        if let Ok(id) = store::add_item(&self.pool, thread, &item).await {
            item.id = id;
            self.to_watchers(
                thread,
                ServerMsg::Drop {
                    thread,
                    item: LIVE_ASSISTANT,
                },
            );
            self.to_watchers(thread, ServerMsg::Item { thread, item });
        }
    }

    /// Writes the streamed thinking segment down as a finished row, when there is one.
    ///
    /// Written like a tool row, so "thought for a while" stays in the scrollback after the turn
    /// settles and the whole thought can still be opened later.
    async fn cut_thinking(&mut self, thread: ThreadId) {
        let Some(turn) = self.turns.get_mut(&thread) else {
            return;
        };
        // A segment with no text is still a thought: a model that keeps its reasoning back
        // opens the block and says nothing, and "thought for a while" is still worth a row.
        if turn.thinking.is_empty() && turn.thinking_since.is_none() {
            return;
        }
        let now = zdt_core::state::now_ms();
        let mut item = TimelineItem {
            kind: ItemKind::Thinking,
            text: std::mem::take(&mut turn.thinking),
            done: true,
            at_ms: now,
            elapsed_ms: turn.thinking_since.take().map_or(0, |since| now - since),
            ..TimelineItem::default()
        };
        if let Ok(id) = store::add_item(&self.pool, thread, &item).await {
            item.id = id;
            self.to_watchers(
                thread,
                ServerMsg::Drop {
                    thread,
                    item: LIVE_THINKING,
                },
            );
            self.to_watchers(thread, ServerMsg::Item { thread, item });
        }
    }

    /// Writes a settled turn down and tells everyone.
    async fn settle(&mut self, thread: ThreadId, error: Option<String>) {
        // What streamed becomes the message, kept even when the turn broke: a half answer worth
        // interrupting for is a half answer worth reading.
        self.cut_thinking(thread).await;
        self.cut_assistant(thread).await;
        let turn = self.turns.remove(&thread).and_then(|live| live.turn);
        // Asks stay open past the turn: a background subagent asks whenever it likes, and an
        // ask wiped here would leave it waiting on an answer nobody can give any more.
        let running = self.runners.contains_key(&thread);
        // A turn parked behind runners earlier closes first, oldest first.
        if let Some(parked) = self.parked.remove(&thread)
            && (!running || turn.is_some())
        {
            self.close_turn(thread, parked).await;
        }
        match turn {
            // Runners keep working past the turn's end. The after checkpoint and the diff wait
            // for the last of them, so their changes land on this turn's card.
            Some(turn) if running => {
                self.parked.insert(thread, turn);
            }
            Some(turn) => self.close_turn(thread, turn).await,
            None => {}
        }
        let state = if error.is_some() {
            ThreadState::Failed
        } else {
            ThreadState::Idle
        };
        let _ = store::set_state(&self.pool, thread, state, error.as_deref()).await;

        // A turn that ended while nobody looked is news to read later; a broken one also wakes
        // a snoozed thread, because a failure must surface.
        let watched = self
            .clients
            .values()
            .any(|client| client.watching == Some(thread));
        if !watched {
            let _ = store::set_unread(&self.pool, thread, true).await;
        }
        if error.is_some() {
            let _ = store::set_snoozed(&self.pool, thread, 0).await;
        }

        // A thread still called by the placeholder names itself off its first prompt.
        if error.is_none() && self.tuning.titles {
            let row = store::thread_row(&self.pool, thread).await.ok().flatten();
            if let Some(row) = row
                && row.title == "New thread"
            {
                self.generate_title(thread, &row.instance);
            }
        }

        self.broadcast_shells().await;
        self.refresh_watchers(thread).await;
    }

    /// Starts the naming task for `thread`, off the loop.
    fn generate_title(&self, thread: ThreadId, instance: &str) {
        let Some(provider) = self.providers.get(instance) else {
            return;
        };
        let provider = provider.clone();
        let pool = self.pool.clone();
        let to_self = self.to_self.clone();
        let model = self.tuning.title_model.clone();
        tokio::spawn(async move {
            let Ok(Some(prompt)) = store::first_prompt(&pool, thread).await else {
                return;
            };
            let Some(title) = provider.title(&model, &prompt).await else {
                return;
            };
            let _ = to_self.send(Cmd::Named { thread, title });
        });
    }

    /// The naming task's answer: the title, and a real branch name for a temporary one.
    async fn named(&mut self, thread: ThreadId, title: String) {
        let title = clean_title(&title);
        if title.is_empty() {
            return;
        }
        let Ok(Some(row)) = store::thread_row(&self.pool, thread).await else {
            return;
        };
        // A person's own rename in the meantime stands.
        if row.title != "New thread" {
            return;
        }
        let _ = store::set_title(&self.pool, thread, &title).await;
        self.broadcast_shells().await;

        // The temporary branch takes a name a person would write. The rename runs off the loop
        // and reports back; a name already taken leaves the temporary one, which still works.
        if let Some(worktree) = row.worktree
            && row.branch.starts_with("zdt/")
            && row.branch.len() == 12
        {
            let fresh = format!("zdt/{}", slug_of(&title));
            if fresh == row.branch {
                return;
            }
            let from = row.branch;
            let to_self = self.to_self.clone();
            tokio::task::spawn_blocking(move || {
                let Ok(repo) = zdt_git::Repo::open(&worktree) else {
                    return;
                };
                match zdt_git::worktree::rename_branch(&repo, &from, &fresh) {
                    Ok(()) => {
                        let _ = to_self.send(Cmd::Rebranched {
                            thread,
                            branch: fresh,
                        });
                    }
                    Err(error) => {
                        tracing::warn!("thread {thread}: branch not renamed: {error}");
                    }
                }
            });
        }
    }

    /// The housekeeping beat.
    async fn tick(&mut self) {
        // Idle provider sessions are stopped; the thread persists and resumes on the next turn.
        if self.tuning.idle_minutes > 0 {
            let too_long = std::time::Duration::from_secs(self.tuning.idle_minutes * 60);
            let now = std::time::Instant::now();
            let quiet: Vec<ThreadId> = self
                .spoke
                .iter()
                .filter(|(_, at)| now.duration_since(**at) > too_long)
                .map(|(thread, _)| *thread)
                .collect();
            for thread in quiet {
                // A session carrying runners is not idle, however quiet its main agent is.
                if self
                    .runners
                    .get(&thread)
                    .is_some_and(|held| !held.is_empty())
                {
                    continue;
                }
                let row = store::thread_row(&self.pool, thread).await.ok().flatten();
                let busy = row.as_ref().is_some_and(|row| row.state.is_busy());
                if busy {
                    continue;
                }
                if let Some(row) = row
                    && let Some(provider) = self.providers.get(&row.instance)
                {
                    provider.stop(thread).await;
                }
                self.spoke.remove(&thread);
            }
        }

        // Old quiet threads settle themselves.
        if self.tuning.auto_settle_days > 0
            && let Ok(moved) = store::auto_settle(&self.pool, self.tuning.auto_settle_days).await
            && moved > 0
        {
            self.broadcast_shells().await;
        }

        // Raw provider logs are pruned once a day.
        let today = zdt_core::state::now_ms() / 86_400_000;
        if self.tuning.log_days > 0 && today != self.pruned_day {
            self.pruned_day = today;
            let (logs, days) = (self.tuning.logs.clone(), self.tuning.log_days);
            tokio::task::spawn_blocking(move || prune_logs(&logs, days));
        }
    }

    /// Closes a turn: the after checkpoint, the diff row, and the thread's running total.
    async fn close_turn(&mut self, thread: ThreadId, turn: i64) {
        let Ok(Some(row)) = store::thread_row(&self.pool, thread).await else {
            return;
        };
        let Ok(Some(opened)) = store::turn_row(&self.pool, turn).await else {
            return;
        };
        if opened.before_ref.is_empty() {
            return;
        }
        let after = zdt_git::checkpoint::turn_ref(thread.0, turn, "after");
        if capture_checkpoint(row.root.clone(), after.clone())
            .await
            .is_none()
        {
            return;
        }
        let _ = store::set_turn_after(&self.pool, turn, &after).await;

        let files = diff_stats(row.root.clone(), opened.before_ref.clone(), after.clone()).await;
        if !files.is_empty() {
            let diff = TurnDiff {
                turn,
                before: opened.before_ref,
                after: after.clone(),
                files,
            };
            let item = TimelineItem {
                kind: ItemKind::Diff,
                text: diff.summary(),
                detail: diff.encode(),
                done: true,
                ..TimelineItem::default()
            };
            let _ = store::add_item(&self.pool, thread, &item).await;
        }
        self.refresh_diff_stat(thread, &row.root).await;
    }

    /// Recounts what the thread's turns have changed so far: first checkpoint to last.
    async fn refresh_diff_stat(&mut self, thread: ThreadId, root: &Path) {
        let first = store::first_before(&self.pool, thread).await.ok().flatten();
        let last = store::last_after(&self.pool, thread).await.ok().flatten();
        let stat = match (first, last) {
            (Some(first), Some(last)) => {
                let files = diff_stats(root.to_path_buf(), first, last).await;
                DiffStat {
                    files: files.len() as u32,
                    added: files.iter().map(|file| file.added).sum(),
                    removed: files.iter().map(|file| file.removed).sum(),
                }
            }
            _ => DiffStat::default(),
        };
        let _ = store::set_diff_stat(&self.pool, thread, stat).await;
    }

    /// The adapter behind `thread`, found through its stored instance name.
    async fn provider_of(&self, thread: ThreadId) -> Option<&crate::provider::Provider> {
        let row = store::thread_row(&self.pool, thread).await.ok().flatten()?;
        self.providers.get(&row.instance)
    }

    /// The sidebar's list, fresh from the database, with the open asks counted in.
    /// Takes every open ask away, telling whoever shows them. For a session that is gone:
    /// its asks cannot be answered any more.
    async fn clear_asks(&mut self, thread: ThreadId) {
        if self
            .asks
            .remove(&thread)
            .is_some_and(|open| !open.is_empty())
        {
            self.push_asks(thread).await;
        }
    }

    /// Adopts a thread's runner set: watchers get the whole picture, and everyone hears when
    /// the count moved, because the count rides the sidebar's shells.
    async fn set_runners(&mut self, thread: ThreadId, runners: Vec<zdt_agent::runner::Runner>) {
        let count_was = self.runners.get(&thread).map_or(0, Vec::len);
        if runners.is_empty() {
            if count_was == 0 {
                return;
            }
            self.runners.remove(&thread);
        } else {
            self.runners.insert(thread, runners.clone());
        }
        let count_now = runners.len();
        self.to_watchers(thread, ServerMsg::Runners { thread, runners });
        if count_now != count_was {
            self.broadcast_shells().await;
        }
        // The last runner is gone. A turn that settled while they worked closes now, unless a
        // follow-up turn is streaming and closes it when it settles.
        if count_now == 0
            && !self.turns.contains_key(&thread)
            && let Some(parked) = self.parked.remove(&thread)
        {
            self.close_turn(thread, parked).await;
            self.refresh_watchers(thread).await;
        }
    }

    async fn shells(&self) -> ServerMsg {
        let mut threads = store::shells(&self.pool).await.unwrap_or_default();
        for shell in &mut threads {
            shell.asking = self.asks.get(&shell.id).map_or(0, |open| open.len() as u32);
            shell.runners = self
                .runners
                .get(&shell.id)
                .map_or(0, |held| held.len() as u32);
            // What the directory actually has checked out, so a mismatch can be said out loud.
            // Two small file reads, never an opened repository.
            if !shell.branch.is_empty() {
                shell.on_branch = head_branch(&shell.root);
            }
        }
        ServerMsg::Shells { threads }
    }

    /// One thread's whole conversation, written rows first, live streams after.
    async fn detail(&self, thread: ThreadId) -> ServerMsg {
        let mut items = store::items(&self.pool, thread).await.unwrap_or_default();
        if let Some(turn) = self.turns.get(&thread) {
            if !turn.thinking.is_empty() || turn.thinking_since.is_some() {
                items.push(TimelineItem {
                    id: LIVE_THINKING,
                    kind: ItemKind::Thinking,
                    text: turn.thinking.clone(),
                    done: false,
                    // How long it has been going, so a fresh watcher's clock starts true.
                    elapsed_ms: turn
                        .thinking_since
                        .map_or(0, |since| zdt_core::state::now_ms() - since),
                    ..TimelineItem::default()
                });
            }
            if !turn.assistant.is_empty() {
                items.push(TimelineItem {
                    id: LIVE_ASSISTANT,
                    kind: ItemKind::Assistant,
                    text: turn.assistant.clone(),
                    done: false,
                    ..TimelineItem::default()
                });
            }
        }
        ServerMsg::Detail { thread, items }
    }

    // ---- Saying things -----------------------------------------------------------------------

    fn to(&self, id: ClientId, message: ServerMsg) {
        if let Some(client) = self.clients.get(&id) {
            let _ = client.to_client.send(message);
        }
    }

    fn refuse(&self, id: ClientId, reason: &str) {
        self.to(
            id,
            ServerMsg::Refused {
                reason: reason.to_owned(),
            },
        );
    }

    fn error(&self, id: ClientId, thread: Option<ThreadId>, message: &str) {
        self.to(
            id,
            ServerMsg::Error {
                thread,
                message: message.to_owned(),
            },
        );
    }

    fn to_watchers(&self, thread: ThreadId, message: ServerMsg) {
        for client in self.clients.values() {
            if client.watching == Some(thread) {
                let _ = client.to_client.send(message.clone());
            }
        }
    }

    /// Tells watchers what is open, and everyone that the count moved.
    async fn push_asks(&mut self, thread: ThreadId) {
        let asks = self.asks.get(&thread).cloned().unwrap_or_default();
        self.to_watchers(thread, ServerMsg::Asks { thread, asks });
        self.broadcast_shells().await;
    }

    async fn broadcast_shells(&self) {
        let shells = self.shells().await;
        for client in self.clients.values() {
            let _ = client.to_client.send(shells.clone());
        }
    }

    async fn refresh_watchers(&self, thread: ThreadId) {
        if !self
            .clients
            .values()
            .any(|client| client.watching == Some(thread))
        {
            return;
        }
        let detail = self.detail(thread).await;
        for client in self.clients.values() {
            if client.watching == Some(thread) {
                let _ = client.to_client.send(detail.clone());
            }
        }
    }
}

/// Makes a thread's worktree: the fetch when asked, the branch, the checkout.
///
/// Blocking. A fetch that fails falls back to the local base with a note, because working from a
/// stale base beats not working at all.
fn bootstrap_worktree(
    root: &Path,
    parent: &Path,
    spec: &WorktreeSpec,
) -> Result<MadeWorktree, String> {
    let repo = zdt_git::Repo::open(root)
        .map_err(|_| format!("{} is not in a git repository", root.display()))?;
    let base = if spec.base.is_empty() {
        "HEAD".to_owned()
    } else {
        spec.base.clone()
    };

    let mut note = None;
    let mut start = base.clone();
    if spec.from_origin && !spec.base.is_empty() {
        match zdt_git::worktree::fetch(&repo, &spec.base) {
            Ok(()) => start = format!("origin/{}", spec.base),
            Err(error) => note = Some(format!("origin not fetched ({error}); started from {base}")),
        }
    }

    let branch = zdt_git::worktree::temp_branch();
    let repo_name = root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repo".to_owned());
    let spot = parent.join(repo_name).join(branch.replace('/', "-"));
    let made = zdt_git::worktree::add(&repo, &spot, &branch, &start)
        .map_err(|error| format!("worktree not made: {error}"))?;
    Ok(MadeWorktree {
        path: made.path,
        branch: made.branch,
        base,
        note,
    })
}

/// Captures a checkpoint at `reference`, off the loop's thread.
///
/// Nothing when the directory is not a repository or the capture fails; a checkpoint is a
/// convenience, and a turn must run without one.
async fn capture_checkpoint(root: PathBuf, reference: String) -> Option<String> {
    tokio::task::spawn_blocking(move || {
        let repo = zdt_git::Repo::open(&root).ok()?;
        match zdt_git::checkpoint::capture(&repo, &reference, "zdt agent checkpoint") {
            Ok(id) => Some(id),
            Err(error) => {
                tracing::warn!("{reference}: not captured: {error}");
                None
            }
        }
    })
    .await
    .ok()
    .flatten()
}

/// The per-file counts between two checkpoints, off the loop's thread.
async fn diff_stats(root: PathBuf, before: String, after: String) -> Vec<FileStat> {
    tokio::task::spawn_blocking(move || {
        let Ok(repo) = zdt_git::Repo::open(&root) else {
            return Vec::new();
        };
        let Ok(files) = zdt_git::checkpoint::changes(&repo, &before, &after) else {
            return Vec::new();
        };
        files
            .into_iter()
            .map(|file| {
                let (added, removed) = file.counts();
                FileStat {
                    path: file.path,
                    added: added as u32,
                    removed: removed as u32,
                    binary: file.binary,
                }
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

/// One line, without wrapping quotes, short enough for a sidebar row.
fn clean_title(said: &str) -> String {
    let line = said
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    let line = line.trim().trim_matches(['"', '\'', '`']).trim();
    let mut clipped: String = line.chars().take(60).collect();
    if clipped.len() < line.len()
        && let Some(cut) = clipped.rfind(' ')
    {
        clipped.truncate(cut);
    }
    clipped
}

/// The prompt a commit draft is written from: the rules, the files, and the patch.
fn commit_prompt(branch: &str, files: &[FileStat], patch: &str) -> String {
    let summary: String = files
        .iter()
        .map(|file| format!("{} +{} -{}\n", file.path, file.added, file.removed))
        .collect();
    let clipped_summary: String = summary.chars().take(6_000).collect();
    let clipped_patch: String = patch.chars().take(40_000).collect();
    let branch = if branch.is_empty() {
        "(detached)"
    } else {
        branch
    };
    format!(
        "You write concise git commit messages.\n\
         Return a JSON object with keys: subject, body, branch.\n\
         Rules:\n\
         - subject is imperative, at most 72 characters, no trailing period\n\
         - body is an empty string or a few short bullet points\n\
         - branch is a short kebab-case git branch name with a conventional type prefix, \
           like feat/thing, fix/bug, refactor/area, chore/task, or docs/topic\n\
         - capture the primary user-visible or developer-visible change\n\
         - return only the JSON object\n\n\
         Branch: {branch}\n\n\
         Files:\n{clipped_summary}\n\
         Patch:\n{clipped_patch}",
    )
}

/// The subject, body and branch out of a drafted answer.
///
/// Lenient on purpose: the object is cut out from whatever surrounds it, because a chatty model
/// wraps JSON in prose or fences.
fn parse_commit_draft(said: &str) -> Option<(String, String, String)> {
    let start = said.find('{')?;
    let end = said.rfind('}')?;
    let value: serde_json::Value = serde_json::from_str(said.get(start..=end)?).ok()?;
    let subject = value["subject"].as_str().unwrap_or_default().trim();
    if subject.is_empty() {
        return None;
    }
    let body = value["body"].as_str().unwrap_or_default().trim();
    let branch = branch_slug(value["branch"].as_str().unwrap_or_default());
    Some((subject.to_owned(), body.to_owned(), branch))
}

/// A branch-safe name that keeps its type prefix: `feat/thing` stays two slugged parts.
fn branch_slug(said: &str) -> String {
    let said = said.trim();
    if said.is_empty() {
        return String::new();
    }
    match said.split_once('/') {
        Some((kind, rest)) => format!("{}/{}", slug_of(kind), slug_of(rest)),
        None => slug_of(said),
    }
}

/// A branch-safe slug of a title: lowercase words joined by dashes.
fn slug_of(title: &str) -> String {
    let mut slug = String::new();
    for word in title.split(|letter: char| !letter.is_alphanumeric()) {
        if word.is_empty() {
            continue;
        }
        if !slug.is_empty() {
            slug.push('-');
        }
        slug.push_str(&word.to_lowercase());
        if slug.len() >= 40 {
            break;
        }
    }
    if slug.is_empty() {
        "thread".to_owned()
    } else {
        slug
    }
}

/// Deletes raw provider logs older than `days`. Blocking.
fn prune_logs(logs: &Path, days: u32) {
    let Ok(entries) = std::fs::read_dir(logs) else {
        return;
    };
    let cutoff =
        std::time::SystemTime::now() - std::time::Duration::from_secs(u64::from(days) * 86_400);
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|kind| kind != "ndjson") {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .is_ok_and(|touched| touched < cutoff);
        if old && let Err(error) = std::fs::remove_file(&path) {
            tracing::warn!("{}: not pruned: {error}", path.display());
        }
    }
}

/// The branch `root` has checked out, read straight from `HEAD`.
///
/// Two file reads and no opened repository, because this runs on every shells push. Empty for a
/// detached head or anything unreadable.
fn head_branch(root: &Path) -> String {
    let dot = root.join(".git");
    // A worktree's `.git` is a file saying where the real directory is.
    let git_dir = if dot.is_file() {
        match std::fs::read_to_string(&dot) {
            Ok(text) => match text.strip_prefix("gitdir:") {
                Some(spot) => PathBuf::from(spot.trim()),
                None => return String::new(),
            },
            Err(_) => return String::new(),
        }
    } else {
        dot
    };
    let Ok(head) = std::fs::read_to_string(git_dir.join("HEAD")) else {
        return String::new();
    };
    head.trim()
        .strip_prefix("ref: refs/heads/")
        .unwrap_or("")
        .to_owned()
}
