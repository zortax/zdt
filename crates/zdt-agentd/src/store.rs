//! What the daemon writes down.
//!
//! One SQLite file. Every query goes through `sqlx::query!`, so the schema the migrations build
//! is the schema the code compiles against; a query that names a column wrongly fails the build
//! and never a turn.

use std::path::{Path, PathBuf};

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use zdt_agent::mode::RuntimeMode;
use zdt_agent::thread::{
    DiffStat, ItemKind, ItemStatus, ThreadId, ThreadShell, ThreadState, TimelineItem, ToolKind,
    Usage,
};
use zdt_agent::todo::Todo;

/// Opens the database at `path`, making it and its schema when they are not there.
pub async fn open(path: &Path) -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await?;
    sqlx::migrate!().run(&pool).await?;
    Ok(pool)
}

/// Marks every thread whose session a restart took as failed.
///
/// The conversation itself is intact and resumes on the next prompt; only the live process is
/// gone, and saying so beats a "working" row that never moves again. Rows still running when the
/// daemon fell are closed the same way.
pub async fn reconcile(pool: &SqlitePool) -> anyhow::Result<()> {
    let now = zdt_core::state::now_ms() as i64;
    sqlx::query!(
        "UPDATE threads
         SET state = 'failed',
             last_error = 'the session did not survive a restart; send a message to continue',
             updated_at_ms = ?
         WHERE state IN ('starting', 'working')",
        now,
    )
    .execute(pool)
    .await?;
    sqlx::query!("UPDATE messages SET status = 'failed' WHERE status = 'running'")
        .execute(pool)
        .await?;
    Ok(())
}

/// The project for `root`, made when it is new.
pub async fn ensure_project(pool: &SqlitePool, root: &str, name: &str) -> anyhow::Result<i64> {
    let now = zdt_core::state::now_ms() as i64;
    let row = sqlx::query!(
        r#"INSERT INTO projects (root, name, created_at_ms)
           VALUES (?, ?, ?)
           ON CONFLICT (root) DO UPDATE SET name = excluded.name
           RETURNING id AS "id!: i64""#,
        root,
        name,
        now,
    )
    .fetch_one(pool)
    .await?;
    Ok(row.id)
}

/// How a new thread works in a worktree, when it does.
#[derive(Default)]
pub struct WorktreeCols {
    /// The worktree's checkout directory.
    pub path: String,
    /// The branch checked out in it.
    pub branch: String,
    /// The branch it started from.
    pub base: String,
}

/// A new idle thread under `project`, driven by the named provider instance.
pub async fn create_thread(
    pool: &SqlitePool,
    project: i64,
    title: &str,
    mode: RuntimeMode,
    worktree: &WorktreeCols,
    instance: &str,
    provider: &str,
) -> anyhow::Result<ThreadId> {
    let now = zdt_core::state::now_ms() as i64;
    let word = mode.word();
    let row = sqlx::query!(
        r#"INSERT INTO threads
               (project_id, title, state, mode, worktree, branch, base_branch,
                instance, provider, created_at_ms, updated_at_ms)
           VALUES (?, ?, 'idle', ?, ?, ?, ?, ?, ?, ?, ?)
           RETURNING id AS "id!: i64""#,
        project,
        title,
        word,
        worktree.path,
        worktree.branch,
        worktree.base,
        instance,
        provider,
        now,
        now,
    )
    .fetch_one(pool)
    .await?;
    Ok(ThreadId(row.id))
}

/// Every thread, newest first, as the sidebar lists them.
///
/// The ask count is the engine's to fill: asks live with the session and never in the file.
pub async fn shells(pool: &SqlitePool) -> anyhow::Result<Vec<ThreadShell>> {
    let rows = sqlx::query!(
        "SELECT threads.id, threads.title, threads.state, threads.last_error,
                threads.mode, threads.model, threads.proposed_plan,
                threads.worktree, threads.branch, threads.instance, threads.provider,
                threads.diff_files, threads.diff_added, threads.diff_removed,
                threads.context_tokens, threads.context_limit, threads.cost_usd,
                threads.pinned, threads.snoozed_until, threads.settled, threads.archived,
                threads.unread, threads.draft, threads.effort,
                threads.created_at_ms, threads.updated_at_ms,
                projects.root, projects.name
         FROM threads
         JOIN projects ON projects.id = threads.project_id
         ORDER BY threads.id DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let project_root = PathBuf::from(row.root);
            let in_worktree = !row.worktree.is_empty();
            ThreadShell {
                id: ThreadId(row.id),
                root: if in_worktree {
                    PathBuf::from(row.worktree)
                } else {
                    project_root.clone()
                },
                project_root,
                project: row.name,
                worktree: in_worktree,
                branch: row.branch,
                on_branch: String::new(),
                changed: DiffStat {
                    files: row.diff_files as u32,
                    added: row.diff_added as u32,
                    removed: row.diff_removed as u32,
                },
                instance: row.instance,
                provider: row.provider,
                title: row.title,
                pinned: row.pinned,
                snoozed_until: row.snoozed_until as u64,
                settled: row.settled != 0,
                archived: row.archived != 0,
                unread: row.unread != 0,
                draft: row.draft,
                state: ThreadState::named(&row.state),
                last_error: row.last_error,
                mode: RuntimeMode::named(&row.mode),
                model: row.model,
                effort: row.effort,
                asking: 0,
                runners: 0,
                planned: row.proposed_plan.is_some(),
                usage: Usage {
                    context_tokens: row.context_tokens as u64,
                    context_limit: row.context_limit as u64,
                    cost_usd: row.cost_usd,
                },
                created_at_ms: row.created_at_ms as u64,
                updated_at_ms: row.updated_at_ms as u64,
            }
        })
        .collect())
}

/// What sending into a thread needs to know.
pub struct ThreadRow {
    /// The directory the agent works in: the worktree when there is one.
    pub root: PathBuf,
    /// The project's own directory, where the repository's main checkout is.
    pub project_root: PathBuf,
    /// The worktree's checkout directory, when the thread has one.
    pub worktree: Option<PathBuf>,
    /// The branch the thread works on. Empty for a thread in the main checkout.
    pub branch: String,
    /// The provider's name for the conversation, when there was one.
    pub resume: Option<String>,
    /// Where the thread stands.
    pub state: ThreadState,
    /// How much its agent may do unasked.
    pub mode: RuntimeMode,
    /// Which model it talks to.
    pub model: String,
    /// How hard its agent reasons. Empty means the provider's default.
    pub effort: String,
    /// Which configured provider instance drives it.
    pub instance: String,
    /// The proposed plan waiting on a person, when one is.
    pub proposed_plan: Option<String>,
    /// What the thread is called.
    pub title: String,
}

/// The row for `thread`, when there is one.
pub async fn thread_row(pool: &SqlitePool, thread: ThreadId) -> anyhow::Result<Option<ThreadRow>> {
    let row = sqlx::query!(
        "SELECT projects.root, threads.resume, threads.state, threads.mode, threads.model,
                threads.effort, threads.instance, threads.proposed_plan, threads.worktree,
                threads.branch, threads.title
         FROM threads
         JOIN projects ON projects.id = threads.project_id
         WHERE threads.id = ?",
        thread.0,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| {
        let project_root = PathBuf::from(row.root);
        let worktree = (!row.worktree.is_empty()).then(|| PathBuf::from(row.worktree));
        ThreadRow {
            root: worktree.clone().unwrap_or_else(|| project_root.clone()),
            project_root,
            worktree,
            branch: row.branch,
            resume: row.resume,
            state: ThreadState::named(&row.state),
            mode: RuntimeMode::named(&row.mode),
            model: row.model,
            effort: row.effort,
            instance: row.instance,
            proposed_plan: row.proposed_plan,
            title: row.title,
        }
    }))
}

/// Moves `thread` to `state`.
pub async fn set_state(
    pool: &SqlitePool,
    thread: ThreadId,
    state: ThreadState,
    last_error: Option<&str>,
) -> anyhow::Result<()> {
    let now = zdt_core::state::now_ms() as i64;
    let word = state.word();
    sqlx::query!(
        "UPDATE threads SET state = ?, last_error = ?, updated_at_ms = ? WHERE id = ?",
        word,
        last_error,
        now,
        thread.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Writes down the provider's name for `thread`'s conversation. Nothing forgets it, which
/// reverting to before the first turn does.
pub async fn set_resume(
    pool: &SqlitePool,
    thread: ThreadId,
    resume: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query!(
        "UPDATE threads SET resume = ? WHERE id = ?",
        resume,
        thread.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Moves `thread` to `mode`.
pub async fn set_mode(
    pool: &SqlitePool,
    thread: ThreadId,
    mode: RuntimeMode,
) -> anyhow::Result<()> {
    let word = mode.word();
    sqlx::query!("UPDATE threads SET mode = ? WHERE id = ?", word, thread.0)
        .execute(pool)
        .await?;
    Ok(())
}

/// Points `thread` at `model`.
pub async fn set_model(pool: &SqlitePool, thread: ThreadId, model: &str) -> anyhow::Result<()> {
    sqlx::query!("UPDATE threads SET model = ? WHERE id = ?", model, thread.0)
        .execute(pool)
        .await?;
    Ok(())
}

/// Sets how hard `thread`'s agent reasons.
pub async fn set_effort(pool: &SqlitePool, thread: ThreadId, effort: &str) -> anyhow::Result<()> {
    sqlx::query!(
        "UPDATE threads SET effort = ? WHERE id = ?",
        effort,
        thread.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Writes down `thread`'s checklist.
pub async fn set_todos(pool: &SqlitePool, thread: ThreadId, todos: &[Todo]) -> anyhow::Result<()> {
    let text = serde_json::to_string(todos).unwrap_or_else(|_| "[]".to_owned());
    sqlx::query!("UPDATE threads SET todos = ? WHERE id = ?", text, thread.0)
        .execute(pool)
        .await?;
    Ok(())
}

/// The checklist `thread` last had.
pub async fn todos(pool: &SqlitePool, thread: ThreadId) -> anyhow::Result<Vec<Todo>> {
    let row = sqlx::query!("SELECT todos FROM threads WHERE id = ?", thread.0)
        .fetch_optional(pool)
        .await?;
    Ok(row
        .and_then(|row| serde_json::from_str(&row.todos).ok())
        .unwrap_or_default())
}

/// Writes down, or takes away, `thread`'s proposed plan.
pub async fn set_plan(
    pool: &SqlitePool,
    thread: ThreadId,
    markdown: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query!(
        "UPDATE threads SET proposed_plan = ? WHERE id = ?",
        markdown,
        thread.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Writes down what `thread`'s conversation weighs.
pub async fn set_context(
    pool: &SqlitePool,
    thread: ThreadId,
    tokens: u64,
    limit: u64,
) -> anyhow::Result<()> {
    let tokens = tokens as i64;
    let limit = limit as i64;
    sqlx::query!(
        "UPDATE threads SET context_tokens = ?, context_limit = MAX(context_limit, ?) WHERE id = ?",
        tokens,
        limit,
        thread.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Adds one turn's cost to `thread`'s total.
pub async fn add_cost(pool: &SqlitePool, thread: ThreadId, cost_usd: f64) -> anyhow::Result<()> {
    sqlx::query!(
        "UPDATE threads SET cost_usd = cost_usd + ? WHERE id = ?",
        cost_usd,
        thread.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Appends one finished row to `thread`'s conversation, answering its id.
pub async fn add_item(
    pool: &SqlitePool,
    thread: ThreadId,
    item: &TimelineItem,
) -> anyhow::Result<i64> {
    let now = zdt_core::state::now_ms() as i64;
    let kind = item.kind.word();
    let tool = item.tool.word();
    let status = item.status.word();
    let elapsed = item.elapsed_ms as i64;
    let row = sqlx::query!(
        r#"INSERT INTO messages
               (thread_id, role, text, name, tool, status, detail, created_at_ms, elapsed_ms)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
           RETURNING id AS "id!: i64""#,
        thread.0,
        kind,
        item.text,
        item.name,
        tool,
        status,
        item.detail,
        now,
        elapsed,
    )
    .fetch_one(pool)
    .await?;
    Ok(row.id)
}

/// Moves a written row: a tool that finished, with what it said.
pub async fn update_item(
    pool: &SqlitePool,
    id: i64,
    text: &str,
    status: ItemStatus,
    detail: &str,
) -> anyhow::Result<()> {
    let word = status.word();
    sqlx::query!(
        "UPDATE messages SET text = ?, status = ?, detail = ? WHERE id = ?",
        text,
        word,
        detail,
        id,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// The whole written-down conversation of `thread`, oldest first.
pub async fn items(pool: &SqlitePool, thread: ThreadId) -> anyhow::Result<Vec<TimelineItem>> {
    let rows = sqlx::query!(
        "SELECT id, role, text, name, tool, status, detail, created_at_ms, elapsed_ms
         FROM messages WHERE thread_id = ? ORDER BY id",
        thread.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| TimelineItem {
            id: row.id,
            kind: ItemKind::named(&row.role),
            text: row.text,
            name: row.name,
            tool: ToolKind::named(&row.tool),
            status: ItemStatus::named(&row.status),
            detail: row.detail,
            done: true,
            at_ms: row.created_at_ms as u64,
            elapsed_ms: row.elapsed_ms as u64,
        })
        .collect())
}

/// Takes `thread` away, history included.
pub async fn delete_thread(pool: &SqlitePool, thread: ThreadId) -> anyhow::Result<()> {
    sqlx::query!("DELETE FROM messages WHERE thread_id = ?", thread.0)
        .execute(pool)
        .await?;
    sqlx::query!("DELETE FROM turns WHERE thread_id = ?", thread.0)
        .execute(pool)
        .await?;
    sqlx::query!("DELETE FROM threads WHERE id = ?", thread.0)
        .execute(pool)
        .await?;
    Ok(())
}

// ---- Turns and checkpoints -------------------------------------------------------------------

/// One turn, as revert reads it back.
pub struct TurnRow {
    /// Which thread it ran in.
    pub thread: ThreadId,
    /// The first message row it wrote: the prompt that started it.
    pub first_item: i64,
    /// The provider's resume cursor from before the turn.
    pub resume_before: Option<String>,
    /// The checkpoint captured before it ran.
    pub before_ref: String,
}

/// Writes a turn down as it starts, answering its id.
pub async fn add_turn(
    pool: &SqlitePool,
    thread: ThreadId,
    first_item: i64,
    resume_before: Option<&str>,
    before_ref: &str,
) -> anyhow::Result<i64> {
    let now = zdt_core::state::now_ms() as i64;
    let row = sqlx::query!(
        r#"INSERT INTO turns (thread_id, first_item, resume_before, before_ref, created_at_ms)
           VALUES (?, ?, ?, ?, ?)
           RETURNING id AS "id!: i64""#,
        thread.0,
        first_item,
        resume_before,
        before_ref,
        now,
    )
    .fetch_one(pool)
    .await?;
    Ok(row.id)
}

/// Writes down the checkpoint a turn started from, once the capture lands.
pub async fn set_turn_before(pool: &SqlitePool, turn: i64, before_ref: &str) -> anyhow::Result<()> {
    sqlx::query!(
        "UPDATE turns SET before_ref = ? WHERE id = ?",
        before_ref,
        turn,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Every turn of `thread` from `turn` onward, oldest first.
pub async fn turns_from(
    pool: &SqlitePool,
    thread: ThreadId,
    turn: i64,
) -> anyhow::Result<Vec<i64>> {
    let rows = sqlx::query!(
        "SELECT id FROM turns WHERE thread_id = ? AND id >= ? ORDER BY id",
        thread.0,
        turn,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|row| row.id).collect())
}

/// Writes down the checkpoint a settled turn ended at.
pub async fn set_turn_after(pool: &SqlitePool, turn: i64, after_ref: &str) -> anyhow::Result<()> {
    sqlx::query!(
        "UPDATE turns SET after_ref = ? WHERE id = ?",
        after_ref,
        turn
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// The turn under `turn`, when there is one.
pub async fn turn_row(pool: &SqlitePool, turn: i64) -> anyhow::Result<Option<TurnRow>> {
    let row = sqlx::query!(
        "SELECT thread_id, first_item, resume_before, before_ref FROM turns WHERE id = ?",
        turn,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| TurnRow {
        thread: ThreadId(row.thread_id),
        first_item: row.first_item,
        resume_before: row.resume_before,
        before_ref: row.before_ref,
    }))
}

/// The first checkpoint `thread` ever captured, where its whole diff starts.
pub async fn first_before(pool: &SqlitePool, thread: ThreadId) -> anyhow::Result<Option<String>> {
    let row = sqlx::query!(
        "SELECT before_ref FROM turns WHERE thread_id = ? AND before_ref != '' ORDER BY id LIMIT 1",
        thread.0,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| row.before_ref))
}

/// The last checkpoint `thread` settled at, where its whole diff ends.
pub async fn last_after(pool: &SqlitePool, thread: ThreadId) -> anyhow::Result<Option<String>> {
    let row = sqlx::query!(
        "SELECT after_ref FROM turns
         WHERE thread_id = ? AND after_ref != '' ORDER BY id DESC LIMIT 1",
        thread.0,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| row.after_ref))
}

/// Forgets `turn` and every turn after it, which reverting to before `turn` does.
pub async fn delete_turns_from(
    pool: &SqlitePool,
    thread: ThreadId,
    turn: i64,
) -> anyhow::Result<()> {
    sqlx::query!(
        "DELETE FROM turns WHERE thread_id = ? AND id >= ?",
        thread.0,
        turn,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Forgets every message from `item` onward.
pub async fn delete_items_from(
    pool: &SqlitePool,
    thread: ThreadId,
    item: i64,
) -> anyhow::Result<()> {
    sqlx::query!(
        "DELETE FROM messages WHERE thread_id = ? AND id >= ?",
        thread.0,
        item,
    )
    .execute(pool)
    .await?;
    Ok(())
}

// ---- Lifecycle overlays ----------------------------------------------------------------------

/// Puts `thread` at `order` among the pinned threads. Zero unpins it.
pub async fn set_pinned(pool: &SqlitePool, thread: ThreadId, order: f64) -> anyhow::Result<()> {
    sqlx::query!(
        "UPDATE threads SET pinned = ? WHERE id = ?",
        order,
        thread.0
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Puts `thread` to sleep until `until_ms`. Zero wakes it.
pub async fn set_snoozed(pool: &SqlitePool, thread: ThreadId, until_ms: u64) -> anyhow::Result<()> {
    let until = until_ms as i64;
    sqlx::query!(
        "UPDATE threads SET snoozed_until = ? WHERE id = ?",
        until,
        thread.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Puts `thread` away as done, or takes it back out.
pub async fn set_settled(pool: &SqlitePool, thread: ThreadId, settled: bool) -> anyhow::Result<()> {
    sqlx::query!(
        "UPDATE threads SET settled = ? WHERE id = ?",
        settled,
        thread.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Archives `thread`, or brings it back.
pub async fn set_archived(
    pool: &SqlitePool,
    thread: ThreadId,
    archived: bool,
) -> anyhow::Result<()> {
    sqlx::query!(
        "UPDATE threads SET archived = ? WHERE id = ?",
        archived,
        thread.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Marks `thread` read or unread.
pub async fn set_unread(pool: &SqlitePool, thread: ThreadId, unread: bool) -> anyhow::Result<()> {
    sqlx::query!(
        "UPDATE threads SET unread = ? WHERE id = ?",
        unread,
        thread.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Calls `thread` by `title`.
pub async fn set_title(pool: &SqlitePool, thread: ThreadId, title: &str) -> anyhow::Result<()> {
    sqlx::query!("UPDATE threads SET title = ? WHERE id = ?", title, thread.0)
        .execute(pool)
        .await?;
    Ok(())
}

/// Points `thread` at the branch its worktree now has checked out.
pub async fn set_branch(pool: &SqlitePool, thread: ThreadId, branch: &str) -> anyhow::Result<()> {
    sqlx::query!(
        "UPDATE threads SET branch = ? WHERE id = ?",
        branch,
        thread.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Keeps the prompt typed into `thread`'s composer and not sent yet.
pub async fn set_draft(pool: &SqlitePool, thread: ThreadId, text: &str) -> anyhow::Result<()> {
    sqlx::query!("UPDATE threads SET draft = ? WHERE id = ?", text, thread.0)
        .execute(pool)
        .await?;
    Ok(())
}

/// Settles every idle thread nobody has touched for `days`, answering how many moved.
///
/// Pinned and archived threads stay where they are, and so does anything busy, asking, or
/// already settled.
pub async fn auto_settle(pool: &SqlitePool, days: u32) -> anyhow::Result<u64> {
    let cutoff = zdt_core::state::now_ms() as i64 - i64::from(days) * 86_400_000;
    let done = sqlx::query!(
        "UPDATE threads SET settled = 1
         WHERE settled = 0 AND archived = 0 AND pinned = 0
           AND state IN ('idle', 'failed') AND updated_at_ms < ?",
        cutoff,
    )
    .execute(pool)
    .await?;
    Ok(done.rows_affected())
}

/// One matching line per thread whose conversation contains `query`, newest thread first.
pub async fn search_messages(
    pool: &SqlitePool,
    query: &str,
    most: i64,
) -> anyhow::Result<Vec<zdt_agent::protocol::FoundRow>> {
    let like = format!(
        "%{}%",
        query
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let rows = sqlx::query!(
        r#"SELECT messages.thread_id AS "thread_id!: i64", MAX(messages.id) AS "id!: i64",
                  messages.text AS "text!: String",
                  threads.title AS "title!: String", projects.name AS "name!: String"
           FROM messages
           JOIN threads ON threads.id = messages.thread_id
           JOIN projects ON projects.id = threads.project_id
           WHERE messages.text LIKE ? ESCAPE '\' AND messages.role IN ('user', 'assistant')
           GROUP BY messages.thread_id
           ORDER BY messages.thread_id DESC
           LIMIT ?"#,
        like,
        most,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| zdt_agent::protocol::FoundRow {
            thread: ThreadId(row.thread_id),
            title: row.title,
            project: row.name,
            snippet: snippet_of(&row.text, query),
        })
        .collect())
}

/// The line of `text` that contains `query`, clipped to fit a picker row.
fn snippet_of(text: &str, query: &str) -> String {
    let lower = text.to_lowercase();
    let needle = query.to_lowercase();
    let at = lower.find(&needle).unwrap_or(0);
    let line_start = text[..at].rfind('\n').map_or(0, |found| found + 1);
    let line_end = text[at..].find('\n').map_or(text.len(), |found| at + found);
    let line = text[line_start..line_end].trim();
    let mut clipped: String = line.chars().take(96).collect();
    if clipped.len() < line.len() {
        clipped.push('\u{2026}');
    }
    clipped
}

/// Reads `thread`: clears its unread mark, and a snooze whose moment has passed.
///
/// Answers whether anything changed, so a watch that changed nothing broadcasts nothing.
pub async fn clear_attention(pool: &SqlitePool, thread: ThreadId) -> anyhow::Result<bool> {
    let now = zdt_core::state::now_ms() as i64;
    let done = sqlx::query!(
        "UPDATE threads
         SET unread = 0,
             snoozed_until = CASE
                 WHEN snoozed_until != 0 AND snoozed_until <= ? THEN 0
                 ELSE snoozed_until
             END
         WHERE id = ?
           AND (unread = 1 OR (snoozed_until != 0 AND snoozed_until <= ?))",
        now,
        thread.0,
        now,
    )
    .execute(pool)
    .await?;
    Ok(done.rows_affected() > 0)
}

/// Every resume cursor any thread holds, so an import list can leave out what is already here.
pub async fn resumes(pool: &SqlitePool) -> anyhow::Result<std::collections::HashSet<String>> {
    let rows =
        sqlx::query!(r#"SELECT resume AS "resume!: String" FROM threads WHERE resume IS NOT NULL"#)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|row| row.resume).collect())
}

/// The first user prompt of `thread`, where a generated title starts from.
pub async fn first_prompt(pool: &SqlitePool, thread: ThreadId) -> anyhow::Result<Option<String>> {
    let row = sqlx::query!(
        "SELECT text FROM messages
         WHERE thread_id = ? AND role = 'user' ORDER BY id LIMIT 1",
        thread.0,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| row.text))
}

/// Writes down what the thread's turns have changed so far.
pub async fn set_diff_stat(
    pool: &SqlitePool,
    thread: ThreadId,
    stat: DiffStat,
) -> anyhow::Result<()> {
    let files = stat.files as i64;
    let added = stat.added as i64;
    let removed = stat.removed as i64;
    sqlx::query!(
        "UPDATE threads SET diff_files = ?, diff_added = ?, diff_removed = ? WHERE id = ?",
        files,
        added,
        removed,
        thread.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}
