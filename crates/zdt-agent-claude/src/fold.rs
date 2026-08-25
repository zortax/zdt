//! What one line from the CLI means.
//!
//! Pure over its own state: values in, events and answers out. The session task feeds it what
//! the pipe says, and the tests feed it captured transcripts. Unknown message types fall through
//! quietly, because the protocol grows with every CLI release and a message this build has no
//! word for is not an error.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use zdt_agent::ask::{Ask, AskKind, Question, QuestionOption};
use zdt_agent::catalog::{Catalog, ModelChoice, SlashCommand};
use zdt_agent::event::{Activity, AgentEvent, StreamKind, WorkItem};
use zdt_agent::thread::{ItemKind, ItemStatus, ThreadId, ToolKind};
use zdt_agent::todo::{Todo, TodoState};

use crate::tools;

/// What the CLI is told when a proposed plan is captured.
const PLAN_CAPTURED: &str = "The client captured your proposed plan. Stop here and wait for the \
                             user's feedback or implementation request in a later turn.";

/// The name of the initialize request every session sends at spawn. Its answer carries the
/// commands and the models the session offers.
pub const INITIALIZE_ID: &str = "zdt-init";

/// One permission request the CLI still waits on.
///
/// Held apart from the folder because the answer comes from another task: the folder notes the
/// ask while reading the pipe, and the adapter builds the response when the person decides.
pub struct PendingAsk {
    /// The id of the tool use the ask is about.
    pub tool_use_id: String,
    /// The tool's name.
    pub tool_name: String,
    /// The input, echoed back verbatim on an allow.
    pub input: Value,
    /// What the CLI suggested an "always" should write down.
    pub suggestions: Vec<Value>,
}

/// The asks a session has open, shared between the reader and the adapter.
pub type Pending = Arc<Mutex<HashMap<String, PendingAsk>>>;

/// What one line folded into.
#[derive(Default)]
pub struct Fold {
    /// What the daemon is told.
    pub events: Vec<AgentEvent>,
    /// What goes straight back onto the CLI's stdin.
    pub writes: Vec<Value>,
}

/// One in-flight tool call, as the stream told of it.
struct Call {
    name: String,
    kind: ItemKind,
    tool: ToolKind,
    summary: String,
}

/// One session's read of the stream.
pub struct Folder {
    /// Which thread the session serves.
    thread: ThreadId,
    /// Whether a turn is between its prompt and its result.
    in_turn: bool,
    /// Every tool call the stream has named, by its id.
    calls: HashMap<String, Call>,
    /// The plans already captured, so the two capture paths do not double up.
    plans: HashSet<String>,
    /// The permission requests the CLI waits on.
    pending: Pending,
    /// The background tasks the session runs beside its turns.
    tasks: crate::tasks::Tasks,
}

impl Folder {
    /// A folder for `thread`, with no turn running, noting asks into `pending`.
    #[must_use]
    pub fn new(thread: ThreadId, pending: Pending) -> Self {
        Self {
            thread,
            in_turn: false,
            calls: HashMap::new(),
            plans: HashSet::new(),
            pending,
            tasks: crate::tasks::Tasks::default(),
        }
    }

    /// Says a prompt was just written, so an ended stream is a broken turn.
    pub fn turn_started(&mut self) {
        self.in_turn = true;
    }

    /// What `value` means.
    pub fn take(&mut self, value: &Value) -> Fold {
        let thread = self.thread;
        match value.get("type").and_then(Value::as_str) {
            Some("system") => match value.get("subtype").and_then(Value::as_str) {
                Some("init") => {
                    let session = text(value, "session_id");
                    let model = text(value, "model");
                    let skills = value
                        .get("skills")
                        .and_then(Value::as_array)
                        .map(|list| {
                            list.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_owned)
                                .collect::<Vec<String>>()
                        })
                        .unwrap_or_default();
                    let mut events = vec![
                        AgentEvent::SessionStarted {
                            thread,
                            session,
                            model,
                        },
                        AgentEvent::State {
                            thread,
                            activity: Activity::Running,
                        },
                    ];
                    if !skills.is_empty() {
                        events.push(AgentEvent::Catalog {
                            thread,
                            catalog: Catalog {
                                skills,
                                ..Catalog::default()
                            },
                        });
                    }
                    Fold {
                        events,
                        writes: Vec::new(),
                    }
                }
                Some(
                    subtype @ ("task_started"
                    | "task_progress"
                    | "task_updated"
                    | "task_notification"
                    | "background_tasks_changed"),
                ) => Fold {
                    events: self
                        .tasks
                        .take(subtype, value)
                        .map(|runners| AgentEvent::Runners { thread, runners })
                        .into_iter()
                        .collect(),
                    writes: Vec::new(),
                },
                _ => Fold::default(),
            },
            Some("stream_event") => self.stream_event(value),
            Some("assistant") => self.assistant(value),
            Some("user") => self.tool_results(value),
            Some("result") => self.result(value),
            Some("control_request") => self.control_request(value),
            Some("control_response") => self.control_response(value),
            Some("control_cancel_request") => {
                let id = text(value, "request_id");
                let held = self
                    .pending
                    .lock()
                    .expect("the ask map is never poisoned")
                    .remove(&id);
                Fold {
                    events: held
                        .map(|_| AgentEvent::AskGone { thread, id })
                        .into_iter()
                        .collect(),
                    writes: Vec::new(),
                }
            }
            _ => Fold::default(),
        }
    }

    /// What a raw API event carries.
    ///
    /// Only the top-level conversation: a subagent's narration names its parent tool use, and
    /// letting it through would interleave every subagent into one transcript.
    fn stream_event(&mut self, value: &Value) -> Fold {
        if value
            .get("parent_tool_use_id")
            .is_some_and(|held| !held.is_null())
        {
            return Fold::default();
        }
        let Some(event) = value.get("event") else {
            return Fold::default();
        };
        match event.get("type").and_then(Value::as_str) {
            Some("content_block_delta") => {
                let Some(delta) = event.get("delta") else {
                    return Fold::default();
                };
                let (kind, held) = match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => (StreamKind::Assistant, delta.get("text")),
                    Some("thinking_delta") => (StreamKind::Thinking, delta.get("thinking")),
                    _ => return Fold::default(),
                };
                let Some(piece) = held.and_then(Value::as_str) else {
                    return Fold::default();
                };
                if piece.is_empty() {
                    return Fold::default();
                }
                Fold {
                    events: vec![AgentEvent::Delta {
                        thread: self.thread,
                        kind,
                        text: piece.to_owned(),
                    }],
                    writes: Vec::new(),
                }
            }
            // A tool block opening is the first anyone hears of the call: the name is known and
            // the input still streams. The row starts here and fills in as the input lands.
            Some("content_block_start") => {
                let Some(block) = event.get("content_block") else {
                    return Fold::default();
                };
                // A thinking block opening says a thought began, even when the model keeps the
                // text itself back. An empty delta carries exactly that.
                if matches!(
                    block.get("type").and_then(Value::as_str),
                    Some("thinking" | "redacted_thinking")
                ) {
                    return Fold {
                        events: vec![AgentEvent::Delta {
                            thread: self.thread,
                            kind: StreamKind::Thinking,
                            text: String::new(),
                        }],
                        writes: Vec::new(),
                    };
                }
                if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                    return Fold::default();
                }
                let id = text(block, "id");
                let name = text(block, "name");
                if id.is_empty() || name.is_empty() || quiet_tool(&name) {
                    return Fold::default();
                }
                let call = Call {
                    kind: work_kind(&name),
                    tool: tools::classify(&name),
                    summary: String::new(),
                    name,
                };
                let item = self.work(&id, &call, ItemStatus::Running, String::new());
                self.calls.insert(id, call);
                Fold {
                    events: vec![item],
                    writes: Vec::new(),
                }
            }
            // The running total, told between messages.
            Some("message_delta") => {
                let Some(usage) = event.get("usage") else {
                    return Fold::default();
                };
                Fold {
                    events: usage_event(self.thread, usage).into_iter().collect(),
                    writes: Vec::new(),
                }
            }
            _ => Fold::default(),
        }
    }

    /// A finished assistant message: the tool inputs are whole now.
    fn assistant(&mut self, value: &Value) -> Fold {
        if value
            .get("parent_tool_use_id")
            .is_some_and(|held| !held.is_null())
        {
            return Fold::default();
        }
        let blocks = value
            .get("message")
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array);
        let Some(blocks) = blocks else {
            return Fold::default();
        };
        let mut events = Vec::new();
        for block in blocks {
            if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                continue;
            }
            let id = text(block, "id");
            let name = text(block, "name");
            let input = block.get("input").cloned().unwrap_or(Value::Null);

            if name == "TodoWrite" {
                events.push(AgentEvent::Todos {
                    thread: self.thread,
                    todos: todos(&input),
                });
                continue;
            }
            if name == "ExitPlanMode" {
                if let Some(markdown) = plan_markdown(&input)
                    && self.plans.insert(id.clone())
                {
                    events.push(AgentEvent::PlanProposed {
                        thread: self.thread,
                        markdown,
                    });
                }
                continue;
            }
            if quiet_tool(&name) {
                continue;
            }
            let summary = tools::summarize(&name, &input);
            let call = self.calls.entry(id.clone()).or_insert_with(|| Call {
                kind: work_kind(&name),
                tool: tools::classify(&name),
                summary: String::new(),
                name: name.clone(),
            });
            call.summary = summary;
            let call = &self.calls[&id];
            events.push(self.work(&id, call, ItemStatus::Running, String::new()));
        }
        Fold {
            events,
            writes: Vec::new(),
        }
    }

    /// A user message off the pipe: tool results, and nothing else worth keeping.
    fn tool_results(&mut self, value: &Value) -> Fold {
        if value
            .get("parent_tool_use_id")
            .is_some_and(|held| !held.is_null())
        {
            return Fold::default();
        }
        let blocks = value
            .get("message")
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array);
        let Some(blocks) = blocks else {
            return Fold::default();
        };
        let mut events = Vec::new();
        for block in blocks {
            if block.get("type").and_then(Value::as_str) != Some("tool_result") {
                continue;
            }
            let id = text(block, "tool_use_id");
            let Some(call) = self.calls.get(&id) else {
                continue;
            };
            let failed = block.get("is_error").and_then(Value::as_bool) == Some(true);
            let output = block.get("content").map(result_text).unwrap_or_default();
            let status = if failed {
                ItemStatus::Failed
            } else {
                ItemStatus::Ok
            };
            events.push(self.work(&id, call, status, tools::clip(&output, 16 * 1024)));
        }
        Fold {
            events,
            writes: Vec::new(),
        }
    }

    /// The turn's end.
    fn result(&mut self, value: &Value) -> Fold {
        self.in_turn = false;
        let thread = self.thread;
        let is_error = value
            .get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let error = is_error.then(|| {
            value
                .get("result")
                .and_then(Value::as_str)
                .unwrap_or("the turn failed")
                .to_owned()
        });
        let cost_usd = value.get("total_cost_usd").and_then(Value::as_f64);
        let mut events = Vec::new();
        if let Some(usage) = value.get("usage")
            && let Some(event) = usage_event(thread, usage)
        {
            events.push(event);
        }
        events.push(AgentEvent::TurnDone {
            thread,
            error,
            cost_usd,
        });
        events.push(AgentEvent::State {
            thread,
            activity: Activity::Idle,
        });
        Fold {
            events,
            writes: Vec::new(),
        }
    }

    /// An answer to something this side asked. Only the initialize answer carries news: the
    /// commands and the models the session offers.
    fn control_response(&mut self, value: &Value) -> Fold {
        let Some(response) = value.get("response") else {
            return Fold::default();
        };
        if response.get("request_id").and_then(Value::as_str) != Some(INITIALIZE_ID) {
            return self.control_refused(response);
        }
        let Some(inner) = response.get("response") else {
            return Fold::default();
        };
        let commands = inner
            .get("commands")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .map(|command| SlashCommand {
                        name: text(command, "name"),
                        description: text(command, "description"),
                    })
                    .filter(|command| !command.name.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let models = inner
            .get("models")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .map(|model| ModelChoice {
                        id: text(model, "value"),
                        label: text(model, "displayName"),
                        description: text(model, "description"),
                    })
                    .filter(|model| !model.id.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let catalog = Catalog {
            commands,
            models,
            skills: Vec::new(),
            efforts: effort_levels(),
        };
        if catalog.is_empty() {
            return Fold::default();
        }
        Fold {
            events: vec![AgentEvent::Catalog {
                thread: self.thread,
                catalog,
            }],
            writes: Vec::new(),
        }
    }

    /// A control request of ours the CLI refused: said out loud, never swallowed.
    ///
    /// The one refusal with a fallback is the `auto` permission mode, which some models do not
    /// carry: the session drops to accept-edits, so the thread keeps working with the nearest
    /// mode there is instead of quietly asking for everything.
    fn control_refused(&mut self, response: &Value) -> Fold {
        if response.get("subtype").and_then(Value::as_str) != Some("error") {
            return Fold::default();
        }
        let error = text(response, "error");
        if error.is_empty() {
            return Fold::default();
        }
        let mut fold = Fold::default();
        if error.contains("auto mode unavailable") {
            fold.writes.push(json!({
                "type": "control_request",
                "request_id": "zdt-mode-fallback",
                "request": { "subtype": "set_permission_mode", "mode": "acceptEdits" },
            }));
            fold.events.push(AgentEvent::Noted {
                thread: self.thread,
                message: "auto mode is unavailable for this model; running with accept edits"
                    .to_owned(),
            });
        } else {
            fold.events.push(AgentEvent::Noted {
                thread: self.thread,
                message: error,
            });
        }
        fold
    }

    /// A question from the CLI. Permission asks go to a person; two tools are answered here.
    fn control_request(&mut self, value: &Value) -> Fold {
        let request_id = text(value, "request_id");
        let Some(request) = value.get("request") else {
            return Fold::default();
        };
        if request.get("subtype").and_then(Value::as_str) != Some("can_use_tool") {
            return Fold::default();
        }
        let tool_name = text(request, "tool_name");
        let input = request.get("input").cloned().unwrap_or(Value::Null);
        let tool_use_id = text(request, "tool_use_id");
        let suggestions = request
            .get("permission_suggestions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        // The plan is the answer: it is captured for a person, and the CLI is told to stop.
        if tool_name == "ExitPlanMode" {
            let mut events = Vec::new();
            if let Some(markdown) = plan_markdown(&input)
                && self.plans.insert(if tool_use_id.is_empty() {
                    format!("plan:{markdown}")
                } else {
                    tool_use_id.clone()
                })
            {
                events.push(AgentEvent::PlanProposed {
                    thread: self.thread,
                    markdown,
                });
            }
            return Fold {
                events,
                writes: vec![deny_frame(&request_id, &tool_use_id, PLAN_CAPTURED)],
            };
        }

        let kind = if tool_name == "AskUserQuestion" {
            AskKind::Question {
                questions: questions(&input),
            }
        } else {
            AskKind::Tool {
                tool: tools::classify(&tool_name),
                summary: tools::summarize(&tool_name, &input),
                detail: tools::detail(&input),
                name: tool_name.clone(),
            }
        };
        self.pending
            .lock()
            .expect("the ask map is never poisoned")
            .insert(
                request_id.clone(),
                PendingAsk {
                    tool_use_id,
                    tool_name,
                    input,
                    suggestions,
                },
            );
        Fold {
            events: vec![AgentEvent::Asked {
                thread: self.thread,
                ask: Ask {
                    id: request_id,
                    kind,
                },
            }],
            writes: Vec::new(),
        }
    }

    /// One work row, as an event.
    fn work(&self, id: &str, call: &Call, status: ItemStatus, detail: String) -> AgentEvent {
        AgentEvent::Work {
            thread: self.thread,
            item: WorkItem {
                key: id.to_owned(),
                kind: call.kind,
                name: call.name.clone(),
                tool: call.tool,
                summary: call.summary.clone(),
                status,
                detail,
            },
        }
    }

    /// What an ended stream means: nothing when the session was idle, a broken turn otherwise.
    pub fn ended(&mut self) -> Vec<AgentEvent> {
        let thread = self.thread;
        let mut events = Vec::new();
        for id in self
            .pending
            .lock()
            .expect("the ask map is never poisoned")
            .drain()
            .map(|(id, _)| id)
        {
            events.push(AgentEvent::AskGone { thread, id });
        }
        if self.in_turn {
            self.in_turn = false;
            events.push(AgentEvent::Fatal {
                thread,
                error: "the provider exited mid-turn; send a new message to continue".to_owned(),
            });
        }
        events.push(AgentEvent::State {
            thread,
            activity: Activity::Stopped,
        });
        events
    }
}

// ---- Answers back onto the pipe --------------------------------------------------------------

/// The frame allowing a pending ask, writing the "always" rules down when asked to.
#[must_use]
pub fn allow_frame(request_id: &str, ask: &PendingAsk, always: bool) -> Value {
    let mut response = json!({
        "behavior": "allow",
        "updatedInput": ask.input,
        "toolUseID": ask.tool_use_id,
    });
    if always {
        // The CLI's own suggestions, held to the session so nothing lands in anyone's settings
        // file; a whole-tool rule when it suggested nothing.
        let rules: Vec<Value> = if ask.suggestions.is_empty() {
            vec![json!({
                "type": "addRules",
                "rules": [{ "toolName": ask.tool_name }],
                "behavior": "allow",
                "destination": "session",
            })]
        } else {
            ask.suggestions
                .iter()
                .map(|held| {
                    let mut rule = held.clone();
                    if let Some(map) = rule.as_object_mut() {
                        map.insert("destination".to_owned(), json!("session"));
                    }
                    rule
                })
                .collect()
        };
        response["updatedPermissions"] = Value::Array(rules);
    }
    success_frame(request_id, response)
}

/// The frame declining a pending ask.
#[must_use]
pub fn deny_ask_frame(request_id: &str, ask: &PendingAsk) -> Value {
    deny_frame(
        request_id,
        &ask.tool_use_id,
        "User declined tool execution.",
    )
}

/// The frame answering a question ask: the questions echoed, the chosen labels beside them.
#[must_use]
pub fn answer_frame(request_id: &str, ask: &PendingAsk, answers: &[Vec<String>]) -> Value {
    let mut chosen = serde_json::Map::new();
    let asked = ask
        .input
        .get("questions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (at, question) in asked.iter().enumerate() {
        let name = question
            .get("question")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let Some(taken) = answers.get(at) else {
            continue;
        };
        let multi = question.get("multiSelect").and_then(Value::as_bool) == Some(true);
        let answer = if multi {
            Value::Array(taken.iter().map(|label| json!(label)).collect())
        } else {
            json!(taken.first().cloned().unwrap_or_default())
        };
        chosen.insert(name.to_owned(), answer);
    }
    success_frame(
        request_id,
        json!({
            "behavior": "allow",
            "updatedInput": {
                "questions": asked,
                "answers": chosen,
            },
            "toolUseID": ask.tool_use_id,
        }),
    )
}

fn success_frame(request_id: &str, response: Value) -> Value {
    json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": request_id,
            "response": response,
        },
    })
}

fn deny_frame(request_id: &str, tool_use_id: &str, message: &str) -> Value {
    json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": request_id,
            "response": {
                "behavior": "deny",
                "message": message,
                "toolUseID": tool_use_id,
            },
        },
    })
}

// ---- Small readings --------------------------------------------------------------------------

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// Whether a tool works quietly, outside the timeline.
fn quiet_tool(name: &str) -> bool {
    matches!(name, "TodoWrite" | "ExitPlanMode" | "AskUserQuestion")
}

fn work_kind(name: &str) -> ItemKind {
    if matches!(name, "Task" | "Agent") {
        ItemKind::Task
    } else {
        ItemKind::Tool
    }
}

/// The checklist inside a TodoWrite input.
fn todos(input: &Value) -> Vec<Todo> {
    input
        .get("todos")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|todo| Todo {
                    text: todo
                        .get("content")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|held| !held.is_empty())
                        .unwrap_or("Task")
                        .to_owned(),
                    state: match todo.get("status").and_then(Value::as_str) {
                        Some("completed") => TodoState::Done,
                        Some("in_progress") => TodoState::Active,
                        _ => TodoState::Pending,
                    },
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The effort levels the CLI takes, pinned by its `--effort` flag.
///
/// The initialize answer names no levels, so the list lives here; the provider's own choice
/// leads it.
fn effort_levels() -> Vec<zdt_agent::catalog::EffortChoice> {
    let level = |id: &str, label: &str, description: &str| zdt_agent::catalog::EffortChoice {
        id: id.to_owned(),
        label: label.to_owned(),
        description: description.to_owned(),
    };
    vec![
        level("default", "Default", "The provider's own choice"),
        level("low", "Low", "Quick answers, little reasoning"),
        level("medium", "Medium", "Balanced reasoning"),
        level("high", "High", "Thorough reasoning"),
        level("xhigh", "X-High", "Very thorough reasoning"),
        level("max", "Max", "As much reasoning as the model gives"),
    ]
}

/// The plan inside an ExitPlanMode input.
fn plan_markdown(input: &Value) -> Option<String> {
    input
        .get("plan")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|held| !held.is_empty())
        .map(str::to_owned)
}

/// The questions inside an AskUserQuestion input.
fn questions(input: &Value) -> Vec<Question> {
    input
        .get("questions")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|question| Question {
                    question: text(question, "question"),
                    header: text(question, "header"),
                    multi: question.get("multiSelect").and_then(Value::as_bool) == Some(true),
                    options: question
                        .get("options")
                        .and_then(Value::as_array)
                        .map(|options| {
                            options
                                .iter()
                                .map(|option| QuestionOption {
                                    label: text(option, "label"),
                                    description: text(option, "description"),
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// What the conversation weighs, read off a usage object.
fn usage_event(thread: ThreadId, usage: &Value) -> Option<AgentEvent> {
    let count = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
    let tokens = count("input_tokens")
        + count("cache_creation_input_tokens")
        + count("cache_read_input_tokens")
        + count("output_tokens");
    (tokens > 0).then_some(AgentEvent::Usage {
        thread,
        context_tokens: tokens,
        context_limit: 0,
    })
}

/// A tool result's text: a raw string, blocks joined bare, or a nested `text`.
fn result_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(list) => list.iter().map(result_text).collect(),
        Value::Object(_) => match value.get("text").and_then(Value::as_str) {
            Some(text) => text.to_owned(),
            None => value.get("content").map(result_text).unwrap_or_default(),
        },
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The captured transcript of one real turn.
    const SIMPLE: &str = include_str!("../tests/fixtures/simple.ndjson");

    fn fresh() -> Folder {
        Folder::new(ThreadId(1), Pending::default())
    }

    #[test]
    fn a_refused_auto_mode_falls_back_to_accept_edits_and_says_so() {
        let mut folder = fresh();
        let fold = folder.take(&json!({
            "type": "control_response",
            "response": {
                "subtype": "error",
                "request_id": "zdt-1",
                "error": "Cannot set permission mode to auto: auto mode unavailable for this model",
            },
        }));
        assert_eq!(fold.writes.len(), 1, "one fallback request goes out");
        assert_eq!(
            fold.writes[0]["request"]["mode"].as_str(),
            Some("acceptEdits")
        );
        assert!(matches!(
            fold.events.as_slice(),
            [AgentEvent::Noted { message, .. }] if message.contains("accept edits")
        ));
    }

    #[test]
    fn any_other_control_refusal_is_told_as_a_note() {
        let mut folder = fresh();
        let fold = folder.take(&json!({
            "type": "control_response",
            "response": {
                "subtype": "error",
                "request_id": "zdt-2",
                "error": "no such model",
            },
        }));
        assert!(fold.writes.is_empty());
        assert!(matches!(
            fold.events.as_slice(),
            [AgentEvent::Noted { message, .. }] if message == "no such model"
        ));
    }

    fn folded() -> Vec<AgentEvent> {
        let mut folder = fresh();
        folder.turn_started();
        SIMPLE
            .lines()
            .filter(|line| !line.trim().is_empty())
            .flat_map(|line| {
                let value: Value = serde_json::from_str(line).expect("the fixture is JSON");
                folder.take(&value).events
            })
            .collect()
    }

    #[test]
    fn a_real_turn_folds_into_a_conforming_stream() {
        let events = folded();
        zdt_agent_harness::conformance::check(&events);
        assert_eq!(zdt_agent_harness::conformance::settled_turns(&events), 1);
    }

    #[test]
    fn the_session_is_named_by_the_init_message() {
        let events = folded();
        let Some(AgentEvent::SessionStarted { session, .. }) = events.first() else {
            panic!("the first event names the session");
        };
        assert_eq!(session, "7f2683d1-93d8-4d43-a9ed-04bd383a0f01");
    }

    #[test]
    fn the_assistant_deltas_join_into_the_answer() {
        let text: String = folded()
            .into_iter()
            .filter_map(|event| match event {
                AgentEvent::Delta {
                    kind: StreamKind::Assistant,
                    text,
                    ..
                } => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(text, "hello from fixture");
    }

    #[test]
    fn the_thinking_streams_apart_from_the_answer() {
        let thinking: String = folded()
            .into_iter()
            .filter_map(|event| match event {
                AgentEvent::Delta {
                    kind: StreamKind::Thinking,
                    text,
                    ..
                } => Some(text),
                _ => None,
            })
            .collect();
        assert!(thinking.contains("The user is"));
    }

    #[test]
    fn the_result_ends_the_turn_with_its_cost() {
        let events = folded();
        let done = events.iter().find_map(|event| match event {
            AgentEvent::TurnDone {
                error, cost_usd, ..
            } => Some((error.clone(), *cost_usd)),
            _ => None,
        });
        let (error, cost) = done.expect("the turn ends");
        assert_eq!(error, None);
        assert!(cost.expect("a cost") > 0.0);
    }

    #[test]
    fn a_stream_that_ends_mid_turn_is_a_broken_turn() {
        let mut folder = fresh();
        folder.turn_started();
        let events = folder.ended();
        assert!(matches!(events[0], AgentEvent::Fatal { .. }));
        assert!(matches!(
            events[1],
            AgentEvent::State {
                activity: Activity::Stopped,
                ..
            }
        ));
    }

    #[test]
    fn a_message_type_this_build_has_no_word_for_is_quietly_skipped() {
        let mut folder = fresh();
        let value = serde_json::json!({"type": "prophecy", "subtype": "doom"});
        let fold = folder.take(&value);
        assert!(fold.events.is_empty());
        assert!(fold.writes.is_empty());
    }

    #[test]
    fn a_tool_block_becomes_a_running_row_and_its_result_finishes_it() {
        let mut folder = fresh();
        folder.turn_started();
        let start = json!({"type": "stream_event", "event": {
            "type": "content_block_start",
            "content_block": {"type": "tool_use", "id": "toolu_1", "name": "Bash"},
        }});
        let events = folder.take(&start).events;
        let Some(AgentEvent::Work { item, .. }) = events.first() else {
            panic!("a tool block starts a row");
        };
        assert_eq!(item.status, ItemStatus::Running);
        assert_eq!(item.tool, ToolKind::Execute);

        let whole = json!({"type": "assistant", "message": {"content": [
            {"type": "tool_use", "id": "toolu_1", "name": "Bash", "input": {"command": "ls"}},
        ]}});
        let events = folder.take(&whole).events;
        let Some(AgentEvent::Work { item, .. }) = events.first() else {
            panic!("a whole message fills the summary in");
        };
        assert_eq!(item.summary, "ls");

        let done = json!({"type": "user", "message": {"content": [
            {"type": "tool_result", "tool_use_id": "toolu_1", "content": "a\nb", "is_error": false},
        ]}});
        let events = folder.take(&done).events;
        let Some(AgentEvent::Work { item, .. }) = events.first() else {
            panic!("a result finishes the row");
        };
        assert_eq!(item.status, ItemStatus::Ok);
        assert_eq!(item.detail, "a\nb");
    }

    #[test]
    fn a_todo_write_becomes_the_checklist_and_no_row() {
        let mut folder = fresh();
        let whole = json!({"type": "assistant", "message": {"content": [
            {"type": "tool_use", "id": "toolu_2", "name": "TodoWrite", "input": {"todos": [
                {"content": "first", "status": "completed"},
                {"content": "second", "status": "in_progress"},
            ]}},
        ]}});
        let events = folder.take(&whole).events;
        assert_eq!(events.len(), 1);
        let AgentEvent::Todos { todos, .. } = &events[0] else {
            panic!("a checklist");
        };
        assert_eq!(todos[0].state, TodoState::Done);
        assert_eq!(todos[1].state, TodoState::Active);
    }

    #[test]
    fn a_permission_ask_is_held_and_an_allow_echoes_the_input() {
        let pending = Pending::default();
        let mut folder = Folder::new(ThreadId(1), Arc::clone(&pending));
        let ask = json!({"type": "control_request", "request_id": "req-1", "request": {
            "subtype": "can_use_tool", "tool_name": "Bash",
            "input": {"command": "rm -f x"}, "tool_use_id": "toolu_9",
            "permission_suggestions": [],
        }});
        let events = folder.take(&ask).events;
        let Some(AgentEvent::Asked { ask, .. }) = events.first() else {
            panic!("an ask");
        };
        assert!(matches!(&ask.kind, AskKind::Tool { summary, .. } if summary == "rm -f x"));

        let held = pending.lock().expect("locks");
        let frame = allow_frame("req-1", &held["req-1"], false);
        assert_eq!(
            frame["response"]["response"]["updatedInput"]["command"],
            "rm -f x"
        );
    }

    #[test]
    fn an_always_allow_writes_a_session_rule_down() {
        let ask = PendingAsk {
            tool_use_id: "toolu_9".to_owned(),
            tool_name: "Bash".to_owned(),
            input: json!({"command": "git status"}),
            suggestions: Vec::new(),
        };
        let frame = allow_frame("req-1", &ask, true);
        let rules = &frame["response"]["response"]["updatedPermissions"];
        assert_eq!(rules[0]["destination"], "session");
        assert_eq!(rules[0]["rules"][0]["toolName"], "Bash");
    }

    #[test]
    fn an_exit_plan_mode_is_captured_and_denied_in_one_move() {
        let mut folder = fresh();
        let ask = json!({"type": "control_request", "request_id": "req-2", "request": {
            "subtype": "can_use_tool", "tool_name": "ExitPlanMode",
            "input": {"plan": "# The plan"}, "tool_use_id": "toolu_3",
        }});
        let fold = folder.take(&ask);
        assert!(matches!(
            fold.events.first(),
            Some(AgentEvent::PlanProposed { markdown, .. }) if markdown == "# The plan"
        ));
        assert_eq!(fold.writes.len(), 1);
        assert_eq!(fold.writes[0]["response"]["response"]["behavior"], "deny");
        // The assistant snapshot of the same call does not capture it twice.
        let whole = json!({"type": "assistant", "message": {"content": [
            {"type": "tool_use", "id": "toolu_3", "name": "ExitPlanMode",
             "input": {"plan": "# The plan"}},
        ]}});
        assert!(folder.take(&whole).events.is_empty());
    }

    #[test]
    fn a_question_ask_answers_with_the_labels_beside_the_questions() {
        let ask = PendingAsk {
            tool_use_id: "toolu_4".to_owned(),
            tool_name: "AskUserQuestion".to_owned(),
            input: json!({"questions": [
                {"question": "Which way?", "header": "Way", "multiSelect": false,
                 "options": [{"label": "Left"}, {"label": "Right"}]},
            ]}),
            suggestions: Vec::new(),
        };
        let frame = answer_frame("req-3", &ask, &[vec!["Left".to_owned()]]);
        let inner = &frame["response"]["response"]["updatedInput"];
        assert_eq!(inner["answers"]["Which way?"], "Left");
        assert_eq!(inner["questions"][0]["question"], "Which way?");
    }
}
