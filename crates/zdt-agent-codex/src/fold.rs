//! app-server frames, folded into [`AgentEvent`]s.
//!
//! One folder per session. It reads every line off the process, keeps the little state a
//! translation needs — the live turn, the open asks, which items streamed — and answers with
//! events for the daemon and frames for the process. Pure over values, so the captured
//! transcripts under `tests/fixtures` pin it down.
//!
//! [`AgentEvent`]: zdt_agent::event::AgentEvent

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use zdt_agent::ask::{Ask, AskKind, Question, QuestionOption};
use zdt_agent::catalog::{Catalog, ModelChoice};
use zdt_agent::event::{Activity, AgentEvent, StreamKind, WorkItem};
use zdt_agent::mode::RuntimeMode;
use zdt_agent::thread::{ItemKind, ItemStatus, ThreadId, ToolKind};
use zdt_agent::todo::{Todo, TodoState};

/// The request id of the initialize call.
pub const INITIALIZE_ID: i64 = 1;

/// The request id of the thread open: `thread/start`, or `thread/resume`.
pub const THREAD_ID: i64 = 2;

/// The request id of the model list.
pub const MODELS_ID: i64 = 3;

/// The request id of the fresh start a failed resume falls back to.
pub const THREAD_RETRY_ID: i64 = 4;

/// Where per-frame request ids the folder mints begin, above every fixed one.
pub const DYNAMIC_IDS: i64 = 100;

/// What one ask needs to be answered on the wire.
#[derive(Clone, Debug)]
pub struct OpenAsk {
    /// The JSON-RPC id the answer carries back.
    pub rpc_id: Value,
    /// The question ids, in order, when the ask is a question ask.
    pub question_ids: Vec<String>,
}

/// What the session's methods and its reader both touch.
///
/// Locked briefly and never across an await, from the adapter's side and the folder's.
#[derive(Default)]
pub struct State {
    /// The provider's name for the conversation, once the thread answered.
    pub session: Option<String>,
    /// The turn running now, by the provider's id for it.
    pub turn: Option<String>,
    /// A prompt waiting for the thread to come up.
    pub queued: Option<String>,
    /// How much the agent may do unasked. Every turn start carries it.
    pub mode: RuntimeMode,
    /// The model turns name. Empty means the provider decides.
    pub model: String,
    /// The reasoning effort turns carry. Empty means the model's default.
    pub effort: String,
    /// The open asks, by the id the daemon knows them under.
    pub asks: HashMap<String, OpenAsk>,
    /// Whether a prompt was written and no turn has settled since.
    pub turn_open: bool,
    /// Whether the thread open on the wire is a resume, which a failure falls back from.
    pub resuming: bool,
}

/// The shared state, as both sides hold it.
pub type Shared = Arc<Mutex<State>>;

/// One folded frame: events out, and frames to write back.
#[derive(Default)]
pub struct Fold {
    /// What the daemon is told.
    pub events: Vec<AgentEvent>,
    /// What goes back onto the process's stdin.
    pub writes: Vec<Value>,
}

/// Folds one session's frames.
pub struct Folder {
    thread: ThreadId,
    state: Shared,
    /// The items that streamed deltas, so a completed item is not said twice.
    streamed: HashSet<String>,
    /// Whether assistant prose streamed since the last cut, for the break between messages.
    assistant_flowing: bool,
    /// Whether thinking streamed since the last cut.
    thinking_flowing: bool,
    /// What each file-change item touches, for the ask that comes without its own words.
    changes: HashMap<String, (String, String)>,
    /// Counts the folder's own request ids, above the fixed ones.
    next_id: i64,
}

impl Folder {
    /// A folder for `thread` over `state`.
    #[must_use]
    pub fn new(thread: ThreadId, state: Shared) -> Self {
        Self {
            thread,
            state,
            streamed: HashSet::new(),
            assistant_flowing: false,
            thinking_flowing: false,
            changes: HashMap::new(),
            next_id: DYNAMIC_IDS,
        }
    }

    /// Folds one frame.
    pub fn take(&mut self, value: &Value) -> Fold {
        let mut fold = Fold::default();
        match (value.get("method").and_then(Value::as_str), value.get("id")) {
            (Some(method), Some(id)) => self.request(method, id.clone(), value, &mut fold),
            (Some(method), None) => self.notification(method, value, &mut fold),
            (None, Some(_)) => self.response(value, &mut fold),
            (None, None) => {}
        }
        fold
    }

    /// The stream ended. A turn still open is a broken one.
    #[must_use]
    pub fn ended(&mut self) -> Vec<AgentEvent> {
        let open = {
            let mut state = self.state.lock().expect("the state is never poisoned");
            state.turn = None;
            std::mem::take(&mut state.turn_open)
        };
        let mut events = vec![AgentEvent::State {
            thread: self.thread,
            activity: Activity::Stopped,
        }];
        if open {
            events.push(AgentEvent::Fatal {
                thread: self.thread,
                error: "the provider went away".to_owned(),
            });
        }
        events
    }

    // ---- Requests from the server ------------------------------------------------------------

    /// One server request: an approval, a question, or something to refuse politely.
    fn request(&mut self, method: &str, id: Value, value: &Value, fold: &mut Fold) {
        let params = value.get("params").cloned().unwrap_or(Value::Null);
        match method {
            "item/commandExecution/requestApproval" => {
                let command = pretty_command(params["command"].as_str().unwrap_or_default());
                let reason = params["reason"].as_str().unwrap_or_default();
                let detail = if reason.is_empty() {
                    command.clone()
                } else {
                    format!("{command}\n\n{reason}")
                };
                self.open_ask(
                    id,
                    AskKind::Tool {
                        name: "shell".to_owned(),
                        tool: ToolKind::Execute,
                        summary: command,
                        detail,
                    },
                    Vec::new(),
                    fold,
                );
            }
            "item/fileChange/requestApproval" => {
                let item = params["itemId"].as_str().unwrap_or_default();
                let (summary, detail) = self
                    .changes
                    .get(item)
                    .cloned()
                    .unwrap_or_else(|| ("change files".to_owned(), String::new()));
                self.open_ask(
                    id,
                    AskKind::Tool {
                        name: "edit".to_owned(),
                        tool: ToolKind::Edit,
                        summary,
                        detail,
                    },
                    Vec::new(),
                    fold,
                );
            }
            "item/tool/requestUserInput" => {
                let questions: Vec<Value> =
                    params["questions"].as_array().cloned().unwrap_or_default();
                let ids: Vec<String> = questions
                    .iter()
                    .map(|question| question["id"].as_str().unwrap_or_default().to_owned())
                    .collect();
                let asked: Vec<Question> = questions
                    .iter()
                    .map(|question| Question {
                        question: question["question"].as_str().unwrap_or_default().to_owned(),
                        header: question["header"].as_str().unwrap_or_default().to_owned(),
                        options: question["options"]
                            .as_array()
                            .map(|options| {
                                options
                                    .iter()
                                    .map(|option| QuestionOption {
                                        label: option["label"]
                                            .as_str()
                                            .unwrap_or_default()
                                            .to_owned(),
                                        description: option["description"]
                                            .as_str()
                                            .unwrap_or_default()
                                            .to_owned(),
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                        multi: false,
                    })
                    .collect();
                self.open_ask(id, AskKind::Question { questions: asked }, ids, fold);
            }
            // A request this build has no answer for is refused, so the server never hangs.
            other => {
                tracing::debug!("refusing a server request: {other}");
                fold.writes.push(json!({
                    "id": id,
                    "error": { "code": -32601, "message": "zdt does not answer this" },
                }));
            }
        }
    }

    /// Opens one ask under the JSON-RPC id it is answered with.
    fn open_ask(&mut self, id: Value, kind: AskKind, question_ids: Vec<String>, fold: &mut Fold) {
        let name = format!("codex-{id}");
        self.state
            .lock()
            .expect("the state is never poisoned")
            .asks
            .insert(
                name.clone(),
                OpenAsk {
                    rpc_id: id,
                    question_ids,
                },
            );
        fold.events.push(AgentEvent::Asked {
            thread: self.thread,
            ask: Ask { id: name, kind },
        });
    }

    // ---- Notifications -----------------------------------------------------------------------

    fn notification(&mut self, method: &str, value: &Value, fold: &mut Fold) {
        let params = value.get("params").cloned().unwrap_or(Value::Null);
        match method {
            "turn/started" => {
                if let Some(turn) = params["turn"]["id"].as_str() {
                    self.state.lock().expect("the state is never poisoned").turn =
                        Some(turn.to_owned());
                }
                fold.events.push(AgentEvent::State {
                    thread: self.thread,
                    activity: Activity::Running,
                });
            }
            "turn/completed" => self.turn_completed(&params, fold),
            "item/agentMessage/delta" => {
                self.streamed(&params);
                self.delta(StreamKind::Assistant, params["delta"].as_str(), fold);
            }
            "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => {
                self.streamed(&params);
                self.delta(StreamKind::Thinking, params["delta"].as_str(), fold);
            }
            "item/reasoning/summaryPartAdded" if self.thinking_flowing => {
                self.delta(StreamKind::Thinking, Some("\n\n"), fold);
            }
            "item/started" => self.item(&params, false, fold),
            "item/completed" => self.item(&params, true, fold),
            "turn/plan/updated" => {
                let todos: Vec<Todo> = params["plan"]
                    .as_array()
                    .map(|steps| {
                        steps
                            .iter()
                            .map(|step| Todo {
                                text: step["step"].as_str().unwrap_or_default().to_owned(),
                                state: match step["status"].as_str().unwrap_or_default() {
                                    "inProgress" => TodoState::Active,
                                    "completed" => TodoState::Done,
                                    _ => TodoState::Pending,
                                },
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                fold.events.push(AgentEvent::Todos {
                    thread: self.thread,
                    todos,
                });
            }
            "thread/tokenUsage/updated" => {
                let usage = &params["tokenUsage"];
                fold.events.push(AgentEvent::Usage {
                    thread: self.thread,
                    context_tokens: usage["last"]["totalTokens"].as_u64().unwrap_or(0),
                    context_limit: usage["modelContextWindow"].as_u64().unwrap_or(0),
                });
            }
            "serverRequest/resolved" => {
                let resolved = params["requestId"].clone();
                let gone = {
                    let mut state = self.state.lock().expect("the state is never poisoned");
                    let name = state
                        .asks
                        .iter()
                        .find(|(_, ask)| ask.rpc_id == resolved)
                        .map(|(name, _)| name.clone());
                    if let Some(name) = &name {
                        state.asks.remove(name);
                    }
                    name
                };
                if let Some(id) = gone {
                    fold.events.push(AgentEvent::AskGone {
                        thread: self.thread,
                        id,
                    });
                }
            }
            "error" => {
                let said = params["error"]["message"].as_str().unwrap_or("codex broke");
                if params["willRetry"].as_bool() == Some(true) {
                    tracing::debug!("codex retries: {said}");
                } else {
                    tracing::warn!("codex: {said}");
                }
            }
            // Status noise, account news, raw items: nothing the daemon shows.
            _ => {}
        }
    }

    /// A settled turn: what it ended as, and the seat freed for the next one.
    fn turn_completed(&mut self, params: &Value, fold: &mut Fold) {
        {
            let mut state = self.state.lock().expect("the state is never poisoned");
            state.turn = None;
            state.turn_open = false;
            // A codex turn blocks on its approvals, so one that ended left none open; whatever
            // is still here is stale, and the daemon keeps asks past turns now.
            for (name, _) in state.asks.drain() {
                fold.events.push(AgentEvent::AskGone {
                    thread: self.thread,
                    id: name,
                });
            }
        }
        self.streamed.clear();
        self.assistant_flowing = false;
        self.thinking_flowing = false;
        let error = match params["turn"]["status"].as_str().unwrap_or_default() {
            "failed" => Some(
                params["turn"]["error"]["message"]
                    .as_str()
                    .unwrap_or("the turn failed")
                    .to_owned(),
            ),
            _ => None,
        };
        fold.events.push(AgentEvent::TurnDone {
            thread: self.thread,
            error,
            cost_usd: None,
        });
    }

    /// One streamed piece.
    fn delta(&mut self, kind: StreamKind, text: Option<&str>, fold: &mut Fold) {
        let Some(text) = text.filter(|text| !text.is_empty()) else {
            return;
        };
        match kind {
            StreamKind::Assistant => self.assistant_flowing = true,
            StreamKind::Thinking => self.thinking_flowing = true,
        }
        fold.events.push(AgentEvent::Delta {
            thread: self.thread,
            kind,
            text: text.to_owned(),
        });
    }

    /// Marks the item under `params` as having streamed.
    fn streamed(&mut self, params: &Value) {
        if let Some(id) = params["itemId"].as_str() {
            self.streamed.insert(id.to_owned());
        }
    }

    /// One item starting or finishing, become a work row or a break in the prose.
    fn item(&mut self, params: &Value, done: bool, fold: &mut Fold) {
        let item = &params["item"];
        let id = item["id"].as_str().unwrap_or_default().to_owned();
        match item["type"].as_str().unwrap_or_default() {
            // A fresh message after one that streamed gets a paragraph break; without it two
            // messages would run together in the one live row.
            "agentMessage" => {
                if done {
                    if !self.streamed.contains(&id) {
                        self.delta(StreamKind::Assistant, item["text"].as_str(), fold);
                    }
                } else if self.assistant_flowing {
                    self.delta(StreamKind::Assistant, Some("\n\n"), fold);
                }
            }
            "reasoning" if done && !self.streamed.contains(&id) => {
                let text = joined_reasoning(item);
                if !text.is_empty() {
                    self.delta(StreamKind::Thinking, Some(&text), fold);
                }
            }
            "commandExecution" => {
                self.work_cut();
                let status = match (done, item["status"].as_str().unwrap_or_default()) {
                    (false, _) => ItemStatus::Running,
                    (true, "declined") => ItemStatus::Declined,
                    (true, "failed") => ItemStatus::Failed,
                    (true, _) => match item["exitCode"].as_i64() {
                        Some(0) | None => ItemStatus::Ok,
                        Some(_) => ItemStatus::Failed,
                    },
                };
                fold.events.push(AgentEvent::Work {
                    thread: self.thread,
                    item: WorkItem {
                        key: id,
                        kind: ItemKind::Tool,
                        name: "shell".to_owned(),
                        tool: ToolKind::Execute,
                        summary: pretty_command(item["command"].as_str().unwrap_or_default()),
                        status,
                        detail: item["aggregatedOutput"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned(),
                    },
                });
            }
            "fileChange" => {
                self.work_cut();
                let (summary, detail) = changed_files(item);
                self.changes
                    .insert(id.clone(), (summary.clone(), detail.clone()));
                let status = match (done, item["status"].as_str().unwrap_or_default()) {
                    (false, _) => ItemStatus::Running,
                    (true, "declined") => ItemStatus::Declined,
                    (true, "failed") => ItemStatus::Failed,
                    (true, _) => ItemStatus::Ok,
                };
                fold.events.push(AgentEvent::Work {
                    thread: self.thread,
                    item: WorkItem {
                        key: id,
                        kind: ItemKind::Tool,
                        name: "edit".to_owned(),
                        tool: ToolKind::Edit,
                        summary,
                        status,
                        detail,
                    },
                });
            }
            "mcpToolCall" => {
                self.work_cut();
                let server = item["server"].as_str().unwrap_or_default();
                let tool = item["tool"].as_str().unwrap_or_default();
                let status = match (done, item["status"].as_str().unwrap_or_default()) {
                    (false, _) => ItemStatus::Running,
                    (true, "failed") => ItemStatus::Failed,
                    (true, _) => ItemStatus::Ok,
                };
                fold.events.push(AgentEvent::Work {
                    thread: self.thread,
                    item: WorkItem {
                        key: id,
                        kind: ItemKind::Tool,
                        name: format!("{server}.{tool}"),
                        tool: ToolKind::Mcp,
                        summary: format!("{server}.{tool}"),
                        status,
                        detail: String::new(),
                    },
                });
            }
            "webSearch" => {
                self.work_cut();
                let query = item["query"].as_str().unwrap_or_default();
                fold.events.push(AgentEvent::Work {
                    thread: self.thread,
                    item: WorkItem {
                        key: id,
                        kind: ItemKind::Tool,
                        name: "web_search".to_owned(),
                        tool: ToolKind::Web,
                        summary: if query.is_empty() {
                            "searched the web".to_owned()
                        } else {
                            query.to_owned()
                        },
                        status: if done {
                            ItemStatus::Ok
                        } else {
                            ItemStatus::Running
                        },
                        detail: String::new(),
                    },
                });
            }
            // The echo of the prompt, plan text, raw internals: nothing to add.
            _ => {}
        }
    }

    /// A work row lands between the streams: the daemon cuts them, and the break bookkeeping
    /// here starts over.
    fn work_cut(&mut self) {
        self.assistant_flowing = false;
        self.thinking_flowing = false;
    }

    // ---- Responses ---------------------------------------------------------------------------

    fn response(&mut self, value: &Value, fold: &mut Fold) {
        let id = value.get("id").and_then(Value::as_i64).unwrap_or(-1);
        let result = value.get("result");
        let error = value.get("error");
        match id {
            THREAD_ID | THREAD_RETRY_ID => match result {
                Some(result) => self.thread_opened(result, fold),
                None => {
                    let said = error
                        .and_then(|error| error["message"].as_str())
                        .unwrap_or("the thread did not open");
                    // A conversation the server no longer has starts over; the transcript in
                    // the daemon's own database is what the person keeps.
                    let fallback = {
                        let state = self.state.lock().expect("the state is never poisoned");
                        id == THREAD_ID && state.resuming
                    };
                    if fallback {
                        tracing::warn!("resume failed ({said}); starting fresh");
                        let frame = {
                            let state = self.state.lock().expect("the state is never poisoned");
                            open_thread_frame(THREAD_RETRY_ID, None, &state)
                        };
                        fold.writes.push(frame);
                    } else {
                        fold.events.push(AgentEvent::Fatal {
                            thread: self.thread,
                            error: said.to_owned(),
                        });
                    }
                }
            },
            MODELS_ID => {
                if let Some(result) = result {
                    fold.events.push(AgentEvent::Catalog {
                        thread: self.thread,
                        catalog: model_catalog(result),
                    });
                }
            }
            _ => {}
        }
    }

    /// The thread is up: its name goes out, and the queued prompt goes in.
    fn thread_opened(&mut self, result: &Value, fold: &mut Fold) {
        let session = result["thread"]["id"].as_str().unwrap_or_default();
        let model = result["model"].as_str().unwrap_or_default();
        let queued = {
            let mut state = self.state.lock().expect("the state is never poisoned");
            state.session = Some(session.to_owned());
            state.resuming = false;
            state.queued.take().map(|prompt| {
                let id = self.next_id;
                turn_frame(id, session, &prompt, &state)
            })
        };
        if queued.is_some() {
            self.next_id += 1;
        }
        fold.events.push(AgentEvent::SessionStarted {
            thread: self.thread,
            session: session.to_owned(),
            model: model.to_owned(),
        });
        fold.writes.extend(queued);
    }
}

// ---- Frames written onto the wire ------------------------------------------------------------

/// The initialize request, always first.
#[must_use]
pub fn initialize_frame() -> Value {
    json!({
        "id": INITIALIZE_ID,
        "method": "initialize",
        "params": {
            "clientInfo": { "name": "zdt", "title": "zdt", "version": env!("CARGO_PKG_VERSION") },
            "capabilities": null,
        },
    })
}

/// The initialized notification that follows it.
#[must_use]
pub fn initialized_frame() -> Value {
    json!({ "method": "initialized" })
}

/// The model list request, for the catalog.
#[must_use]
pub fn models_frame() -> Value {
    json!({ "id": MODELS_ID, "method": "model/list", "params": {} })
}

/// The thread open: a resume when `resume` names a conversation, a fresh start otherwise.
#[must_use]
pub fn open_thread_frame(id: i64, resume: Option<&str>, state: &State) -> Value {
    let (policy, sandbox) = mode_words(state.mode);
    let mut params = json!({
        "approvalPolicy": policy,
        "sandbox": sandbox,
    });
    if !state.model.is_empty() {
        params["model"] = json!(state.model);
    }
    match resume {
        Some(thread) => {
            params["threadId"] = json!(thread);
            json!({ "id": id, "method": "thread/resume", "params": params })
        }
        None => json!({ "id": id, "method": "thread/start", "params": params }),
    }
}

/// One turn start. The mode and the model ride along, so a change taken between turns holds.
#[must_use]
pub fn turn_frame(id: i64, session: &str, prompt: &str, state: &State) -> Value {
    let (policy, _) = mode_words(state.mode);
    let mut params = json!({
        "threadId": session,
        "input": [{ "type": "text", "text": prompt, "text_elements": [] }],
        "approvalPolicy": policy,
        "sandboxPolicy": sandbox_policy(state.mode),
    });
    if !state.model.is_empty() {
        params["model"] = json!(state.model);
    }
    if !state.effort.is_empty() {
        params["effort"] = json!(state.effort);
    }
    json!({ "id": id, "method": "turn/start", "params": params })
}

/// A steer into the turn that runs.
#[must_use]
pub fn steer_frame(id: i64, session: &str, turn: &str, prompt: &str) -> Value {
    json!({
        "id": id,
        "method": "turn/steer",
        "params": {
            "threadId": session,
            "input": [{ "type": "text", "text": prompt, "text_elements": [] }],
            "expectedTurnId": turn,
        },
    })
}

/// The interrupt for the turn that runs.
#[must_use]
pub fn interrupt_frame(id: i64, session: &str, turn: &str) -> Value {
    json!({
        "id": id,
        "method": "turn/interrupt",
        "params": { "threadId": session, "turnId": turn },
    })
}

/// The answer to an approval ask.
#[must_use]
pub fn decision_frame(rpc_id: &Value, decision: &zdt_agent::ask::Decision) -> Value {
    use zdt_agent::ask::Decision;
    let word = match decision {
        Decision::Allow => "accept",
        Decision::AllowAlways => "acceptForSession",
        Decision::Deny | Decision::Unknown => "decline",
    };
    json!({ "id": rpc_id, "result": { "decision": word } })
}

/// The answer to a question ask: chosen labels under each question's id.
#[must_use]
pub fn answers_frame(rpc_id: &Value, question_ids: &[String], answers: &[Vec<String>]) -> Value {
    let mut map = serde_json::Map::new();
    for (id, chosen) in question_ids.iter().zip(answers) {
        map.insert(id.clone(), json!({ "answers": chosen }));
    }
    json!({ "id": rpc_id, "result": { "answers": map } })
}

/// What a runtime mode says on the wire: the approval policy, and the thread-level sandbox.
fn mode_words(mode: RuntimeMode) -> (&'static str, &'static str) {
    match mode {
        RuntimeMode::Supervised | RuntimeMode::Unknown => ("untrusted", "read-only"),
        RuntimeMode::AcceptEdits | RuntimeMode::Auto => ("on-request", "workspace-write"),
        RuntimeMode::Full => ("never", "danger-full-access"),
        // Codex has no plan surface of its own; read-only work is the honest translation.
        RuntimeMode::Plan => ("on-request", "read-only"),
    }
}

/// The per-turn sandbox policy for a runtime mode.
fn sandbox_policy(mode: RuntimeMode) -> Value {
    match mode {
        RuntimeMode::Supervised | RuntimeMode::Plan | RuntimeMode::Unknown => {
            json!({ "type": "readOnly" })
        }
        RuntimeMode::AcceptEdits | RuntimeMode::Auto => json!({ "type": "workspaceWrite" }),
        RuntimeMode::Full => json!({ "type": "dangerFullAccess" }),
    }
}

// ---- Little readers --------------------------------------------------------------------------

/// The catalog a model list answers with. The provider's own choice leads it.
///
/// The effort levels come from the same answer: what the default model supports, or the first
/// visible one, each with the server's own description.
fn model_catalog(result: &Value) -> Catalog {
    let mut models = vec![ModelChoice {
        id: "default".to_owned(),
        label: "Default".to_owned(),
        description: "The provider's own choice".to_owned(),
    }];
    let mut efforts = vec![zdt_agent::catalog::EffortChoice {
        id: "default".to_owned(),
        label: "Default".to_owned(),
        description: "The model's own choice".to_owned(),
    }];
    if let Some(data) = result["data"].as_array() {
        models.extend(
            data.iter()
                .filter(|model| model["hidden"].as_bool() != Some(true))
                .map(|model| ModelChoice {
                    id: model["id"].as_str().unwrap_or_default().to_owned(),
                    label: model["displayName"].as_str().unwrap_or_default().to_owned(),
                    description: model["description"].as_str().unwrap_or_default().to_owned(),
                }),
        );
        let leading = data
            .iter()
            .find(|model| model["isDefault"].as_bool() == Some(true))
            .or_else(|| {
                data.iter()
                    .find(|model| model["hidden"].as_bool() != Some(true))
            });
        if let Some(levels) =
            leading.and_then(|model| model["supportedReasoningEfforts"].as_array())
        {
            efforts.extend(levels.iter().filter_map(|level| {
                let id = level["reasoningEffort"].as_str().unwrap_or_default();
                if id.is_empty() {
                    return None;
                }
                let mut label: String = id.to_owned();
                if let Some(first) = label.get_mut(0..1) {
                    first.make_ascii_uppercase();
                }
                Some(zdt_agent::catalog::EffortChoice {
                    id: id.to_owned(),
                    label,
                    description: level["description"].as_str().unwrap_or_default().to_owned(),
                })
            }));
        }
    }
    Catalog {
        models,
        efforts,
        ..Catalog::default()
    }
}

/// A command without its shell wrapper, in one line.
fn pretty_command(command: &str) -> String {
    let mut said = command.trim();
    for prefix in [
        "/bin/zsh -lc ",
        "/bin/bash -lc ",
        "/bin/sh -lc ",
        "bash -lc ",
        "sh -lc ",
    ] {
        if let Some(rest) = said.strip_prefix(prefix) {
            said = rest;
            break;
        }
    }
    let said = said.trim_matches('\'').trim_matches('"');
    said.lines().next().unwrap_or_default().to_owned()
}

/// What a file-change item touches: the paths in one line, and the diffs whole.
fn changed_files(item: &Value) -> (String, String) {
    let changes = item["changes"].as_array().cloned().unwrap_or_default();
    let names: Vec<&str> = changes
        .iter()
        .filter_map(|change| change["path"].as_str())
        .map(|path| path.rsplit('/').next().unwrap_or(path))
        .collect();
    let summary = if names.is_empty() {
        "change files".to_owned()
    } else {
        names.join(", ")
    };
    let detail = changes
        .iter()
        .filter_map(|change| {
            let path = change["path"].as_str()?;
            let diff = change["diff"].as_str().unwrap_or_default();
            Some(format!("--- {path}\n{diff}"))
        })
        .collect::<Vec<String>>()
        .join("\n");
    (summary, detail)
}

/// A reasoning item's text, summary first, joined into one thought.
fn joined_reasoning(item: &Value) -> String {
    let read = |field: &str| -> Vec<String> {
        item[field]
            .as_array()
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut parts = read("summary");
    if parts.is_empty() {
        parts = read("content");
    }
    parts.join("\n\n")
}
