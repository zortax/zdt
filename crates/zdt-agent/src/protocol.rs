//! What crosses the daemon's socket.
//!
//! One connection carries everything: the editor sends commands, and the daemon pushes what
//! changed. A client that reconnects subscribes again and is sent fresh snapshots, so nothing
//! on either side has to remember what the other saw.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ask::{Ask, Decision};
use crate::catalog::Catalog;
use crate::mode::RuntimeMode;
use crate::thread::{ItemKind, ThreadId, ThreadShell, TimelineItem};
use crate::todo::Todo;

/// What the editor asks the daemon.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "msg", rename_all = "snake_case")]
pub enum ClientMsg {
    /// Always first, so a version that cannot be talked to is a clean refusal.
    Hello {
        /// Which conversation the client speaks.
        version: u32,
        /// Which process is asking, for the log.
        pid: u32,
    },
    /// Make a thread that works in `root`.
    Create {
        /// The project directory. With a worktree asked for, the worktree is made beside the
        /// state directory and becomes the thread's working directory instead.
        root: PathBuf,
        /// What to call it. Empty means the daemon names it.
        #[serde(default)]
        title: String,
        /// A worktree of its own to work in, when one is wanted.
        #[serde(default)]
        worktree: Option<WorktreeSpec>,
        /// Which provider instance drives it. Empty means the daemon's default.
        #[serde(default)]
        instance: String,
    },
    /// Send a prompt into a thread.
    Send {
        /// Which thread.
        thread: ThreadId,
        /// What to say.
        text: String,
    },
    /// Stop the turn that is running.
    Interrupt {
        /// Which thread.
        thread: ThreadId,
    },
    /// Decide an open tool ask.
    Decide {
        /// Which thread.
        thread: ThreadId,
        /// Which ask.
        id: String,
        /// The decision.
        decision: Decision,
    },
    /// Answer an open question ask.
    Answer {
        /// Which thread.
        thread: ThreadId,
        /// Which ask.
        id: String,
        /// The chosen option labels, one list per question.
        answers: Vec<Vec<String>>,
    },
    /// Take the proposed plan and have it carried out.
    Implement {
        /// Which thread.
        thread: ThreadId,
    },
    /// Set how much the thread's agent may do unasked.
    SetMode {
        /// Which thread.
        thread: ThreadId,
        /// The mode.
        mode: RuntimeMode,
    },
    /// Set which model the thread talks to.
    SetModel {
        /// Which thread.
        thread: ThreadId,
        /// The model, in the provider's own words. Empty means its default.
        model: String,
    },
    /// Set how hard the thread's agent reasons.
    SetEffort {
        /// Which thread.
        thread: ThreadId,
        /// The level, in the provider's own words. Empty means its default.
        effort: String,
    },
    /// Follow one thread's conversation. Replaces whatever was watched before.
    Watch {
        /// Which thread.
        thread: ThreadId,
    },
    /// Stop following.
    Unwatch,
    /// Put the working tree back to before one turn ran, and forget that turn onward.
    Revert {
        /// Which thread.
        thread: ThreadId,
        /// Which turn, by the daemon's id for it.
        turn: i64,
    },
    /// Commit everything in the thread's working tree.
    Commit {
        /// Which thread.
        thread: ThreadId,
        /// The commit message.
        message: String,
        /// Whether to push the branch afterwards.
        push: bool,
        /// A fresh branch to make at `HEAD` and commit onto. Empty commits where the checkout
        /// stands.
        #[serde(default)]
        branch: String,
        /// The files to take, relative to the repository root. Empty takes everything.
        #[serde(default)]
        paths: Vec<String>,
    },
    /// Scan what a commit would take, and have a message drafted for it.
    ///
    /// Answered twice: [`ServerMsg::CommitFiles`] as soon as the tree is read, and
    /// [`ServerMsg::CommitDraft`] once the model has written.
    DraftCommit {
        /// Which thread.
        thread: ThreadId,
    },
    /// Take the thread away, history included. A worktree thread's worktree goes with it.
    Delete {
        /// Which thread.
        thread: ThreadId,
    },
    /// Give the thread a place among the pinned ones, or take it away.
    Pin {
        /// Which thread.
        thread: ThreadId,
        /// Its place, highest first. Zero unpins.
        order: f64,
    },
    /// Put the thread to sleep until a moment, or wake it.
    Snooze {
        /// Which thread.
        thread: ThreadId,
        /// When the snooze ends, in milliseconds since the epoch. Zero wakes it now.
        until_ms: u64,
    },
    /// Put the thread away as done, or take it back out.
    Settle {
        /// Which thread.
        thread: ThreadId,
        /// Whether it is done.
        settled: bool,
    },
    /// Archive the thread, or bring it back.
    Archive {
        /// Which thread.
        thread: ThreadId,
        /// Whether it is archived.
        archived: bool,
    },
    /// Mark the thread read or unread by hand.
    MarkUnread {
        /// Which thread.
        thread: ThreadId,
        /// Whether it reads as unread.
        unread: bool,
    },
    /// Call the thread something else. An empty title asks the daemon to make one up.
    Rename {
        /// Which thread.
        thread: ThreadId,
        /// The new title, or empty for a generated one.
        title: String,
    },
    /// Keep the prompt typed into the thread's composer and not sent yet.
    SetDraft {
        /// Which thread.
        thread: ThreadId,
        /// The text. Empty forgets it.
        text: String,
    },
    /// Look for threads whose conversation contains the words.
    Search {
        /// What to look for.
        query: String,
    },
    /// List the conversations the named instance's provider already holds on disk.
    ListImports {
        /// Which instance to look under.
        instance: String,
    },
    /// Make a thread out of one of them: its history read in, its resume cursor kept.
    Import {
        /// Which instance to look under.
        instance: String,
        /// The provider's own name for the conversation.
        id: String,
    },
    /// Stop the daemon. Running turns are interrupted.
    Shutdown,
}

/// One provider-side conversation, offered for import.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ImportRow {
    /// The provider's own name for it.
    pub id: String,
    /// What it is called.
    pub title: String,
    /// The directory it worked in.
    pub root: PathBuf,
    /// When it last moved, in milliseconds since the epoch.
    pub at_ms: u64,
}

/// One thread a search found.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FoundRow {
    /// Which thread.
    pub thread: ThreadId,
    /// What it is called.
    pub title: String,
    /// The project it works in.
    pub project: String,
    /// One matching line of its conversation.
    pub snippet: String,
}

/// How a new thread's worktree is made.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WorktreeSpec {
    /// The revision the branch starts from: a branch name, or any commit.
    pub base: String,
    /// Whether to fetch `base` from `origin` first and start from the remote's head.
    pub from_origin: bool,
}

/// What the daemon answers and pushes.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "msg", rename_all = "snake_case")]
pub enum ServerMsg {
    /// The daemon is there and speaks this conversation.
    Welcome {
        /// Which conversation it speaks.
        version: u32,
        /// Which process it is.
        pid: u32,
    },
    /// The daemon will not do it, and why.
    Refused {
        /// What to tell the person.
        reason: String,
    },
    /// Every thread, newest first. Pushed whenever any of them changes shape.
    Shells {
        /// The threads.
        threads: Vec<ThreadShell>,
    },
    /// The thread a `Create` made.
    Created {
        /// Which thread.
        thread: ThreadId,
    },
    /// The watched thread's whole conversation. Sent on watch, and again when a turn settles.
    Detail {
        /// Which thread.
        thread: ThreadId,
        /// The rows, oldest first.
        items: Vec<TimelineItem>,
    },
    /// A piece of streamed text for one row of the watched thread.
    Append {
        /// Which thread.
        thread: ThreadId,
        /// Which row. A row nobody has is made.
        item: i64,
        /// What the row is.
        kind: ItemKind,
        /// The piece.
        text: String,
    },
    /// A live row of the watched thread that is gone: its text was written down under a new id,
    /// or the turn it belonged to settled.
    Drop {
        /// Which thread.
        thread: ThreadId,
        /// Which row.
        item: i64,
    },
    /// One whole row of the watched thread, put in place. A row nobody has is made.
    Item {
        /// Which thread.
        thread: ThreadId,
        /// The row.
        item: TimelineItem,
    },
    /// Everything the watched thread stops to ask. Replaces the last list.
    Asks {
        /// Which thread.
        thread: ThreadId,
        /// The open asks, oldest first.
        asks: Vec<Ask>,
    },
    /// The watched thread's runners: everything working beside its main agent. Replaces the
    /// last set.
    Runners {
        /// Which thread.
        thread: ThreadId,
        /// Everything running now.
        runners: Vec<crate::runner::Runner>,
    },
    /// The watched thread's proposed plan, or its withdrawal.
    Plan {
        /// Which thread.
        thread: ThreadId,
        /// The plan as markdown; nothing when it was taken or dropped.
        markdown: Option<String>,
    },
    /// The watched thread's checklist. Replaces the last list.
    Todos {
        /// Which thread.
        thread: ThreadId,
        /// The steps.
        todos: Vec<Todo>,
    },
    /// What the watched thread's session offers: commands, skills, models.
    Catalog {
        /// Which thread.
        thread: ThreadId,
        /// The whole of what is known.
        catalog: Catalog,
    },
    /// Something a command asked for went wrong.
    Error {
        /// Which thread, when one is involved.
        thread: Option<ThreadId>,
        /// What happened.
        message: String,
    },
    /// Something a command asked for went well, in one line worth showing.
    Note {
        /// Which thread.
        thread: ThreadId,
        /// What happened.
        message: String,
    },
    /// What a search turned up.
    Found {
        /// The words that were looked for.
        query: String,
        /// The threads whose conversation has them, best first.
        rows: Vec<FoundRow>,
    },
    /// The conversations an instance's provider holds on disk, importable.
    Imports {
        /// Which instance was looked under.
        instance: String,
        /// What was found, newest first.
        rows: Vec<ImportRow>,
    },
    /// What a commit of the thread's working tree would take.
    CommitFiles {
        /// Which thread.
        thread: ThreadId,
        /// The files, with their counts.
        files: Vec<crate::change::FileStat>,
    },
    /// A drafted commit message, for a person to read and change.
    CommitDraft {
        /// Which thread.
        thread: ThreadId,
        /// One imperative line.
        subject: String,
        /// The body under it. Empty when the change needs none.
        body: String,
        /// A short branch name for the change.
        branch: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_round_trips() {
        let sent = ClientMsg::Send {
            thread: ThreadId(4),
            text: "hello".to_owned(),
        };
        let text = serde_json::to_string(&sent).expect("it encodes");
        let back: ClientMsg = serde_json::from_str(&text).expect("it decodes");
        assert!(matches!(back, ClientMsg::Send { thread, .. } if thread == ThreadId(4)));
    }

    #[test]
    fn a_field_a_later_release_added_does_not_stop_an_older_daemon_reading_it() {
        let text = r#"{"msg":"create","root":"/x","title":"","run_on_create":"make dev"}"#;
        let back: ClientMsg = serde_json::from_str(text).expect("it decodes anyway");
        assert!(matches!(back, ClientMsg::Create { .. }));
    }
}
