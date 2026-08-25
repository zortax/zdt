//! The background tasks a session runs beside its turns.
//!
//! The CLI tells of them on the `system` channel: `task_started` when one begins,
//! `task_progress` while it works, `task_updated` for status patches, `task_notification` when
//! it ends, and `background_tasks_changed` as a whole replacement set. This folds those into
//! one live list of [`Runner`]s, and answers a fresh snapshot whenever the list changed.
//!
//! Only background work is held. A blocking subagent is already a running row in the timeline,
//! and the main agent is busy for as long as it runs; the runners are what keeps going after
//! the turn ends.

use serde_json::Value;
use zdt_agent::runner::{Runner, RunnerKind, WorkflowAgent, WorkflowRun};

/// The live set, in the order the tasks appeared.
#[derive(Default)]
pub struct Tasks {
    held: Vec<Runner>,
}

impl Tasks {
    /// Folds one `system` line in. Answers the new whole set when it changed.
    pub fn take(&mut self, subtype: &str, value: &Value) -> Option<Vec<Runner>> {
        let changed = match subtype {
            "task_started" => self.started(value),
            "task_progress" => self.progressed(value),
            "task_updated" => self.updated(value),
            "task_notification" => self.remove(&text(value, "task_id")),
            "background_tasks_changed" => self.replaced(value),
            _ => false,
        };
        changed.then(|| self.held.clone())
    }

    fn started(&mut self, value: &Value) -> bool {
        // Work owned by a subagent belongs to that subagent's story.
        if value
            .get("owned_by_subagent")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return false;
        }
        let kind = kind_of(&text(value, "task_type"));
        let background = value
            .get("is_backgrounded")
            .and_then(Value::as_bool)
            .unwrap_or(kind == RunnerKind::Workflow);
        if !background {
            return false;
        }
        let id = text(value, "task_id");
        if id.is_empty() || self.held.iter().any(|runner| runner.id == id) {
            return false;
        }
        let workflow = (kind == RunnerKind::Workflow).then(|| WorkflowRun {
            name: text(value, "workflow_name"),
            ..WorkflowRun::default()
        });
        self.held.push(Runner {
            id,
            kind,
            description: text(value, "description"),
            agent_type: text(value, "subagent_type"),
            background,
            workflow,
            ..Runner::default()
        });
        true
    }

    fn progressed(&mut self, value: &Value) -> bool {
        let id = text(value, "task_id");
        let Some(runner) = self.held.iter_mut().find(|runner| runner.id == id) else {
            return false;
        };
        if let Some(usage) = value.get("usage") {
            runner.tokens = number(usage, "total_tokens");
            runner.tool_uses = number(usage, "tool_uses") as u32;
            runner.duration_ms = number(usage, "duration_ms");
        }
        if let Some(tool) = value.get("last_tool_name").and_then(Value::as_str) {
            runner.last_tool = tool.to_owned();
        }
        if let Some(said) = value.get("summary").and_then(Value::as_str) {
            runner.summary = said.to_owned();
        }
        if let Some(progress) = value.get("workflow_progress").and_then(Value::as_array) {
            let name = runner
                .workflow
                .as_ref()
                .map(|run| run.name.clone())
                .unwrap_or_default();
            runner.workflow = Some(workflow_of(name, progress));
        }
        true
    }

    fn updated(&mut self, value: &Value) -> bool {
        let id = text(value, "task_id");
        let Some(patch) = value.get("patch") else {
            return false;
        };
        if matches!(
            patch.get("status").and_then(Value::as_str),
            Some("completed" | "failed" | "killed")
        ) {
            return self.remove(&id);
        }
        let Some(runner) = self.held.iter_mut().find(|runner| runner.id == id) else {
            return false;
        };
        let mut changed = false;
        if let Some(said) = patch.get("description").and_then(Value::as_str) {
            runner.description = said.to_owned();
            changed = true;
        }
        if let Some(error) = patch.get("error").and_then(Value::as_str) {
            runner.summary = error.to_owned();
            changed = true;
        }
        changed
    }

    /// The CLI's whole replacement set: what it lists lives, what it left out is gone.
    fn replaced(&mut self, value: &Value) -> bool {
        let Some(listed) = value.get("tasks").and_then(Value::as_array) else {
            return false;
        };
        let before = self.held.len();
        let ids: Vec<String> = listed.iter().map(|task| text(task, "task_id")).collect();
        self.held.retain(|runner| ids.contains(&runner.id));
        let mut changed = self.held.len() != before;
        for task in listed {
            let id = text(task, "task_id");
            if id.is_empty() || self.held.iter().any(|runner| runner.id == id) {
                continue;
            }
            let kind = kind_of(&text(task, "task_type"));
            self.held.push(Runner {
                id,
                kind,
                description: text(task, "description"),
                background: true,
                workflow: (kind == RunnerKind::Workflow).then(WorkflowRun::default),
                ..Runner::default()
            });
            changed = true;
        }
        changed
    }

    fn remove(&mut self, id: &str) -> bool {
        let before = self.held.len();
        self.held.retain(|runner| runner.id != id);
        self.held.len() != before
    }
}

/// What a `task_type` word means.
fn kind_of(word: &str) -> RunnerKind {
    match word {
        "local_workflow" => RunnerKind::Workflow,
        "local_bash" => RunnerKind::Shell,
        // The common case, and what an absent field meant in older CLIs.
        "local_agent" | "" => RunnerKind::Agent,
        _ => RunnerKind::Other,
    }
}

/// A workflow's picture out of one `workflow_progress` array.
fn workflow_of(name: String, progress: &[Value]) -> WorkflowRun {
    let mut run = WorkflowRun {
        name,
        ..WorkflowRun::default()
    };
    let mut phases: Vec<(u64, String)> = Vec::new();
    for entry in progress {
        match entry.get("type").and_then(Value::as_str) {
            Some("workflow_phase") => {
                let title = text(entry, "title");
                if !title.is_empty() {
                    phases.push((number(entry, "index"), title));
                }
            }
            Some("workflow_agent") => run.agents.push(WorkflowAgent {
                label: text(entry, "label"),
                phase: text(entry, "phaseTitle"),
                model: text(entry, "model"),
                state: text(entry, "state"),
                tokens: number(entry, "tokens"),
                tool_calls: number(entry, "toolCalls") as u32,
                duration_ms: number(entry, "durationMs"),
                last_tool: text(entry, "lastToolName"),
                last_summary: text(entry, "lastToolSummary"),
            }),
            Some("workflow_log") => {
                let said = text(entry, "message");
                if !said.is_empty() {
                    run.logs.push(said);
                }
            }
            _ => {}
        }
    }
    phases.sort_by_key(|(index, _)| *index);
    run.phases = phases.into_iter().map(|(_, title)| title).collect();
    run
}

/// The string under `key`, owned. Empty when absent.
fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// The number under `key`. Zero when absent.
fn number(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_background_agent_is_tracked_from_start_to_notification() {
        let mut tasks = Tasks::default();
        let started = tasks
            .take(
                "task_started",
                &json!({
                    "task_id": "a1", "description": "Map the crates",
                    "subagent_type": "Explore", "is_backgrounded": true,
                    "task_type": "local_agent"
                }),
            )
            .expect("a start changes the set");
        assert_eq!(started.len(), 1);
        assert_eq!(started[0].kind, RunnerKind::Agent);
        assert_eq!(started[0].agent_type, "Explore");

        let moved = tasks
            .take(
                "task_progress",
                &json!({
                    "task_id": "a1",
                    "usage": {"total_tokens": 1200, "tool_uses": 7, "duration_ms": 9000},
                    "last_tool_name": "Read", "summary": "reading the folder"
                }),
            )
            .expect("progress changes the set");
        assert_eq!(moved[0].tokens, 1200);
        assert_eq!(moved[0].last_tool, "Read");

        let gone = tasks
            .take(
                "task_notification",
                &json!({"task_id": "a1", "status": "completed"}),
            )
            .expect("the end changes the set");
        assert!(gone.is_empty());
    }

    #[test]
    fn a_blocking_task_is_left_out() {
        let mut tasks = Tasks::default();
        let answer = tasks.take(
            "task_started",
            &json!({"task_id": "a2", "description": "sync", "is_backgrounded": false}),
        );
        assert!(answer.is_none());
    }

    #[test]
    fn a_workflow_reads_its_phases_agents_and_logs() {
        let mut tasks = Tasks::default();
        tasks.take(
            "task_started",
            &json!({
                "task_id": "w1", "description": "run the plan",
                "task_type": "local_workflow", "workflow_name": "review-changes"
            }),
        );
        let moved = tasks
            .take(
                "task_progress",
                &json!({
                    "task_id": "w1",
                    "usage": {"total_tokens": 500, "tool_uses": 0, "duration_ms": 100},
                    "workflow_progress": [
                        {"type": "workflow_phase", "index": 2, "title": "Verify"},
                        {"type": "workflow_phase", "index": 1, "title": "Review"},
                        {"type": "workflow_agent", "label": "review:bugs", "phaseTitle": "Review",
                         "model": "claude-opus-5", "state": "progress", "tokens": 300,
                         "toolCalls": 4, "durationMs": 80, "lastToolName": "Grep",
                         "lastToolSummary": "searching the diff"},
                        {"type": "workflow_log", "message": "3 findings so far"}
                    ]
                }),
            )
            .expect("progress changes the set");
        let run = moved[0].workflow.as_ref().expect("a workflow rides along");
        assert_eq!(run.name, "review-changes");
        assert_eq!(run.phases, vec!["Review".to_owned(), "Verify".to_owned()]);
        assert_eq!(run.agents.len(), 1);
        assert_eq!(run.agents[0].label, "review:bugs");
        assert!(run.agents[0].is_live());
        assert_eq!(run.logs, vec!["3 findings so far".to_owned()]);
    }

    #[test]
    fn the_replacement_set_governs_what_lives() {
        let mut tasks = Tasks::default();
        tasks.take(
            "task_started",
            &json!({"task_id": "a1", "description": "one", "is_backgrounded": true}),
        );
        let replaced = tasks
            .take(
                "background_tasks_changed",
                &json!({"tasks": [
                    {"task_id": "b7", "task_type": "local_bash", "description": "cargo build"}
                ]}),
            )
            .expect("the replacement changes the set");
        assert_eq!(replaced.len(), 1);
        assert_eq!(replaced[0].id, "b7");
        assert_eq!(replaced[0].kind, RunnerKind::Shell);
    }

    #[test]
    fn a_terminal_patch_removes_the_runner() {
        let mut tasks = Tasks::default();
        tasks.take(
            "task_started",
            &json!({"task_id": "a1", "description": "one", "is_backgrounded": true}),
        );
        let gone = tasks
            .take(
                "task_updated",
                &json!({"task_id": "a1", "patch": {"status": "failed"}}),
            )
            .expect("a terminal patch changes the set");
        assert!(gone.is_empty());
    }
}
