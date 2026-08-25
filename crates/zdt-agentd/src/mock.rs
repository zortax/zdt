//! A provider that costs nothing.
//!
//! One in-process adapter that streams synthetic turns: prose deltas, thinking, tool rows, asks,
//! failures. It exists to fill the surface for layout and load work without a real agent behind
//! it, and it is only there when an instance names the `mock` provider.
//!
//! # The prompt drives the turn
//!
//! Words in the prompt shape what comes back: `chunks=200` streams that many pieces,
//! `delay=5` waits that many milliseconds between them, `tools=3` runs that many tool rows,
//! `ask` stops for an approval, and `fail` ends the turn badly. Everything else is a default
//! small answer.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;
use tokio::sync::mpsc::UnboundedSender;
use zdt_agent::ask::{Ask, AskKind, Decision};
use zdt_agent::catalog::{Catalog, ModelChoice};
use zdt_agent::event::{Activity, AgentEvent, StreamKind, WorkItem};
use zdt_agent::runner::{Runner, RunnerKind, WorkflowAgent, WorkflowRun};
use zdt_agent::thread::{ItemKind, ItemStatus, ThreadId, ToolKind};
use zdt_agent_harness::{HarnessError, ProviderAdapter, SessionStart};

/// The prose the stream repeats.
const WORDS: &[&str] = &[
    "The", "quick", "brown", "fox", "jumps", "over", "the", "lazy", "dog", "while", "counting",
    "tokens", "and", "keeping", "the", "timeline", "busy", "with", "steady", "text.",
];

/// What one turn was asked to be.
#[derive(Clone, Copy)]
struct Shape {
    /// How many pieces of prose to stream.
    chunks: u32,
    /// How long to wait between pieces, in milliseconds.
    delay_ms: u64,
    /// How many tool rows to run.
    tools: u32,
    /// How many background subagents keep running past the turn.
    agents: u32,
    /// Whether a workflow keeps running past the turn.
    workflow: bool,
    /// Whether to stop for an approval first.
    ask: bool,
    /// Whether to end badly.
    fail: bool,
}

impl Shape {
    /// The shape `text` asks for.
    fn of(text: &str) -> Self {
        let mut shape = Self {
            chunks: 24,
            delay_ms: 10,
            tools: 1,
            agents: 0,
            workflow: false,
            ask: false,
            fail: false,
        };
        for word in text.split_whitespace() {
            if let Some(count) = word.strip_prefix("chunks=") {
                shape.chunks = count.parse().unwrap_or(shape.chunks);
            } else if let Some(wait) = word.strip_prefix("delay=") {
                shape.delay_ms = wait.parse().unwrap_or(shape.delay_ms);
            } else if let Some(count) = word.strip_prefix("tools=") {
                shape.tools = count.parse().unwrap_or(shape.tools);
            } else if let Some(count) = word.strip_prefix("agents=") {
                shape.agents = count.parse().unwrap_or(shape.agents);
            } else if word == "workflow" {
                shape.workflow = true;
            } else if word == "ask" {
                shape.ask = true;
            } else if word == "fail" {
                shape.fail = true;
            }
        }
        shape
    }
}

/// The mock harness.
///
/// Cloning one is cloning a handle: every clone drives the same turns.
#[derive(Clone)]
pub struct MockAdapter {
    inner: Arc<Inner>,
}

struct Inner {
    /// Where everything noticed goes.
    events: UnboundedSender<AgentEvent>,
    /// The running turns: how each one is stopped early, or its ask answered.
    turns: Mutex<HashMap<ThreadId, Running>>,
}

/// One running turn's handles.
struct Running {
    /// Flipping this ends the turn at the next chunk.
    stop: tokio::sync::watch::Sender<bool>,
    /// Where a decision lands while the turn waits on its ask.
    deciding: Option<tokio::sync::oneshot::Sender<Decision>>,
}

impl MockAdapter {
    /// An adapter with no turns, reporting into `events`.
    #[must_use]
    pub fn new(events: UnboundedSender<AgentEvent>) -> Self {
        Self {
            inner: Arc::new(Inner {
                events,
                turns: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Answers a catalog straight away: a mock session has nothing to start.
    pub fn probe(&self, thread: ThreadId) {
        let catalog = Catalog {
            models: vec![
                ModelChoice {
                    id: "default".to_owned(),
                    label: "Default".to_owned(),
                    description: "The provider's own choice".to_owned(),
                },
                ModelChoice {
                    id: "mock-fast".to_owned(),
                    label: "Mock Fast".to_owned(),
                    description: "Streams without waiting".to_owned(),
                },
                ModelChoice {
                    id: "mock-smart".to_owned(),
                    label: "Mock Smart".to_owned(),
                    description: "The same words, slower".to_owned(),
                },
            ],
            efforts: vec![
                zdt_agent::catalog::EffortChoice {
                    id: "default".to_owned(),
                    label: "Default".to_owned(),
                    description: "The model's own choice".to_owned(),
                },
                zdt_agent::catalog::EffortChoice {
                    id: "low".to_owned(),
                    label: "Low".to_owned(),
                    description: "Barely thinks".to_owned(),
                },
                zdt_agent::catalog::EffortChoice {
                    id: "high".to_owned(),
                    label: "High".to_owned(),
                    description: "Thinks hard about the same words".to_owned(),
                },
            ],
            ..Catalog::default()
        };
        let _ = self
            .inner
            .events
            .send(AgentEvent::Catalog { thread, catalog });
    }

    /// A made-up title for `prompt`: its first words, cleaned up.
    #[must_use]
    pub fn title_of(prompt: &str) -> String {
        let words: Vec<&str> = prompt
            .split_whitespace()
            .filter(|word| !word.contains('='))
            .take(5)
            .collect();
        if words.is_empty() {
            "Mock thread".to_owned()
        } else {
            words.join(" ")
        }
    }

    fn say(&self, event: AgentEvent) {
        let _ = self.inner.events.send(event);
    }
}

impl ProviderAdapter for MockAdapter {
    fn kind(&self) -> &'static str {
        "mock"
    }

    async fn send_turn(&self, start: SessionStart, text: String) -> Result<(), HarnessError> {
        let thread = start.thread;
        let shape = Shape::of(&text);
        let (stop, stopped) = tokio::sync::watch::channel(false);
        {
            let mut turns = self.inner.turns.lock().await;
            if turns.contains_key(&thread) {
                // Steering a running mock turn changes nothing; the turn runs its shape out.
                return Ok(());
            }
            turns.insert(
                thread,
                Running {
                    stop,
                    deciding: None,
                },
            );
        }
        let adapter = self.clone();
        tokio::spawn(async move {
            let leftover = adapter.run_turn(thread, shape, stopped.clone()).await;
            // The turn's slot opens before the runners drain, so a new prompt is not swallowed
            // while background work keeps going — exactly the shape the real providers have.
            adapter.inner.turns.lock().await.remove(&thread);
            adapter.drain_runners(thread, leftover, stopped).await;
        });
        Ok(())
    }

    async fn interrupt(&self, thread: ThreadId) -> Result<(), HarnessError> {
        let mut turns = self.inner.turns.lock().await;
        let Some(running) = turns.get_mut(&thread) else {
            return Err(HarnessError::NoSession(thread));
        };
        let _ = running.stop.send(true);
        // An ask still open is withdrawn with the turn.
        if let Some(deciding) = running.deciding.take() {
            let _ = deciding.send(Decision::Deny);
        }
        Ok(())
    }

    async fn decide(
        &self,
        thread: ThreadId,
        _id: String,
        decision: Decision,
    ) -> Result<(), HarnessError> {
        let mut turns = self.inner.turns.lock().await;
        let Some(running) = turns.get_mut(&thread) else {
            return Err(HarnessError::NoSession(thread));
        };
        let Some(deciding) = running.deciding.take() else {
            return Err(HarnessError::Gone("that ask is no longer open".to_owned()));
        };
        let _ = deciding.send(decision);
        Ok(())
    }

    async fn answer(
        &self,
        thread: ThreadId,
        id: String,
        _answers: Vec<Vec<String>>,
    ) -> Result<(), HarnessError> {
        self.decide(thread, id, Decision::Allow).await
    }

    async fn set_mode(
        &self,
        _thread: ThreadId,
        _mode: zdt_agent::mode::RuntimeMode,
    ) -> Result<(), HarnessError> {
        Ok(())
    }

    async fn set_model(&self, _thread: ThreadId, _model: String) -> Result<(), HarnessError> {
        Ok(())
    }

    async fn stop(&self, thread: ThreadId) {
        let _ = self.interrupt(thread).await;
    }

    async fn stop_all(&self) {
        let threads: Vec<ThreadId> = self.inner.turns.lock().await.keys().copied().collect();
        for thread in threads {
            let _ = self.interrupt(thread).await;
        }
    }
}

impl MockAdapter {
    /// One whole turn, from session start to done. Answers the runners left going past it.
    async fn run_turn(
        &self,
        thread: ThreadId,
        shape: Shape,
        mut stopped: tokio::sync::watch::Receiver<bool>,
    ) -> Vec<Runner> {
        let pause = std::time::Duration::from_millis(shape.delay_ms);
        self.say(AgentEvent::SessionStarted {
            thread,
            session: format!("mock-{thread}"),
            model: "mock".to_owned(),
        });
        self.say(AgentEvent::State {
            thread,
            activity: Activity::Running,
        });

        // A short thought first, so the thinking stream is exercised too.
        self.say(AgentEvent::Delta {
            thread,
            kind: StreamKind::Thinking,
            text: "Considering the shape of the answer.".to_owned(),
        });
        tokio::time::sleep(pause).await;

        if shape.ask && !self.stop_asked(&stopped) {
            let ask = Ask {
                id: format!("mock-ask-{thread}"),
                kind: AskKind::Tool {
                    name: "Bash".to_owned(),
                    tool: ToolKind::Execute,
                    summary: "run `echo mock`".to_owned(),
                    detail: "echo mock".to_owned(),
                },
            };
            self.say(AgentEvent::Asked { thread, ask });
            let (deciding, decided) = tokio::sync::oneshot::channel();
            if let Some(running) = self.inner.turns.lock().await.get_mut(&thread) {
                running.deciding = Some(deciding);
            }
            let decision = decided.await.unwrap_or(Decision::Deny);
            self.say(AgentEvent::AskGone {
                thread,
                id: format!("mock-ask-{thread}"),
            });
            let declined = matches!(decision, Decision::Deny);
            self.work(thread, 0, "echo mock", declined);
            if declined {
                self.say(AgentEvent::TurnDone {
                    thread,
                    error: None,
                    cost_usd: None,
                });
                return Vec::new();
            }
        }

        for tool in 0..shape.tools {
            if self.stop_asked(&stopped) {
                break;
            }
            self.work(thread, tool + 1, &format!("mock tool {}", tool + 1), false);
            tokio::time::sleep(pause).await;
        }

        for chunk in 0..shape.chunks {
            if stopped.has_changed().unwrap_or(false) && *stopped.borrow_and_update() {
                break;
            }
            let word = WORDS[chunk as usize % WORDS.len()];
            self.say(AgentEvent::Delta {
                thread,
                kind: StreamKind::Assistant,
                text: format!("{word} "),
            });
            tokio::time::sleep(pause).await;
        }

        self.say(AgentEvent::Usage {
            thread,
            context_tokens: u64::from(shape.chunks) * 4,
            context_limit: 200_000,
        });

        // Runners appear just before the turn ends, so the surface shows a thread whose main
        // agent idles while work keeps going beside it.
        let runners = if self.stop_asked(&stopped) {
            Vec::new()
        } else {
            mock_runners(shape)
        };
        if !runners.is_empty() {
            self.say(AgentEvent::Runners {
                thread,
                runners: runners.clone(),
            });
        }

        self.say(AgentEvent::TurnDone {
            thread,
            error: shape.fail.then(|| "the mock was told to fail".to_owned()),
            cost_usd: None,
        });
        runners
    }

    /// Keeps the runners moving past their turn, then drains them one by one. A stop clears
    /// them at the next beat.
    async fn drain_runners(
        &self,
        thread: ThreadId,
        mut runners: Vec<Runner>,
        stopped: tokio::sync::watch::Receiver<bool>,
    ) {
        if runners.is_empty() {
            return;
        }
        let beat = std::time::Duration::from_millis(40 * 30);
        let mut step = 0u32;
        while !runners.is_empty() {
            tokio::time::sleep(beat).await;
            if self.stop_asked(&stopped) {
                runners.clear();
            } else {
                step += 1;
                advance_runners(&mut runners, step);
            }
            self.say(AgentEvent::Runners {
                thread,
                runners: runners.clone(),
            });
        }
    }

    /// One finished tool row.
    fn work(&self, thread: ThreadId, index: u32, summary: &str, declined: bool) {
        self.say(AgentEvent::Work {
            thread,
            item: WorkItem {
                key: format!("mock-work-{thread}-{index}"),
                kind: ItemKind::Tool,
                name: "Bash".to_owned(),
                tool: ToolKind::Execute,
                summary: summary.to_owned(),
                detail: "mock output".to_owned(),
                status: if declined {
                    ItemStatus::Declined
                } else {
                    ItemStatus::Ok
                },
            },
        });
    }

    /// Whether the turn was asked to stop.
    fn stop_asked(&self, stopped: &tokio::sync::watch::Receiver<bool>) -> bool {
        *stopped.borrow()
    }
}

/// The runners a shape asks for, fresh at their start.
fn mock_runners(shape: Shape) -> Vec<Runner> {
    let mut runners = Vec::new();
    for index in 0..shape.agents {
        runners.push(Runner {
            id: format!("mock-agent-{index}"),
            kind: RunnerKind::Agent,
            description: format!("Explore corner {}", index + 1),
            agent_type: "Explore".to_owned(),
            background: true,
            ..Runner::default()
        });
    }
    if shape.workflow {
        runners.push(Runner {
            id: "mock-workflow".to_owned(),
            kind: RunnerKind::Workflow,
            description: "Run the mock plan".to_owned(),
            background: true,
            workflow: Some(WorkflowRun {
                name: "mock-plan".to_owned(),
                phases: vec!["Scan".to_owned(), "Fix".to_owned(), "Verify".to_owned()],
                agents: (0..4)
                    .map(|index| WorkflowAgent {
                        label: format!("scan:area-{}", index + 1),
                        phase: "Scan".to_owned(),
                        model: "mock-fast".to_owned(),
                        state: "start".to_owned(),
                        ..WorkflowAgent::default()
                    })
                    .collect(),
                logs: vec!["scanning 4 areas".to_owned()],
            }),
            ..Runner::default()
        });
    }
    runners
}

/// Moves every runner along one beat, and drops the ones whose story ended.
fn advance_runners(runners: &mut Vec<Runner>, step: u32) {
    runners.retain_mut(|runner| {
        runner.tokens += 700;
        runner.tool_uses += 2;
        runner.duration_ms += 800;
        runner.last_tool = ["Read", "Grep", "Bash", "Edit"][step as usize % 4].to_owned();
        match runner.kind {
            // A plain agent works a few beats and finishes, later ones later.
            RunnerKind::Agent => {
                let ordinal: u32 = runner
                    .id
                    .rsplit('-')
                    .next()
                    .and_then(|word| word.parse().ok())
                    .unwrap_or(0);
                runner.summary = format!("reading area {}", step);
                step < ordinal + 4
            }
            RunnerKind::Workflow => {
                let Some(run) = runner.workflow.as_mut() else {
                    return false;
                };
                for (index, agent) in run.agents.iter_mut().enumerate() {
                    let turns = step.saturating_sub(index as u32);
                    agent.state = match turns {
                        0 => "start",
                        1 | 2 => "progress",
                        _ => "done",
                    }
                    .to_owned();
                    if agent.is_live() {
                        agent.tokens += 900;
                        agent.tool_calls += 3;
                        agent.duration_ms += 800;
                        agent.last_tool = "Grep".to_owned();
                        agent.last_summary = format!("sweeping step {step}");
                    }
                }
                if step == 4 {
                    run.logs.push("scan complete, fixing".to_owned());
                }
                runner.summary = format!("Scan: {} of 4 done", step.min(4));
                step < 8
            }
            RunnerKind::Shell | RunnerKind::Other => step < 4,
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use zdt_agent::mode::RuntimeMode;

    /// Runs one turn to its end and collects everything said.
    async fn run(text: &str, decide: Option<Decision>) -> Vec<AgentEvent> {
        let (events, mut inbox) = tokio::sync::mpsc::unbounded_channel();
        let adapter = MockAdapter::new(events);
        let start = SessionStart {
            thread: ThreadId(7),
            cwd: std::env::temp_dir(),
            resume: None,
            model: String::new(),
            effort: String::new(),
            mode: RuntimeMode::Supervised,
        };
        adapter
            .send_turn(start, text.to_owned())
            .await
            .expect("the mock takes every turn");
        let mut said = Vec::new();
        while let Some(event) = inbox.recv().await {
            if let AgentEvent::Asked {
                thread, ref ask, ..
            } = event
                && let Some(decision) = decide.clone()
            {
                let (thread, id) = (thread, ask.id.clone());
                let deciding = adapter.clone();
                tokio::spawn(async move {
                    let _ = deciding.decide(thread, id, decision).await;
                });
            }
            let done = matches!(event, AgentEvent::TurnDone { .. });
            said.push(event);
            if done {
                break;
            }
        }
        said
    }

    #[tokio::test]
    async fn a_shaped_turn_streams_and_settles_cleanly() {
        let said = run("chunks=5 delay=0 tools=2", None).await;
        zdt_agent_harness::conformance::check(&said);
        assert_eq!(zdt_agent_harness::conformance::settled_turns(&said), 1);
        let words = said
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    AgentEvent::Delta {
                        kind: StreamKind::Assistant,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(words, 5);
    }

    #[tokio::test]
    async fn an_ask_waits_for_its_decision_and_a_decline_ends_the_turn() {
        let said = run("ask delay=0", Some(Decision::Deny)).await;
        zdt_agent_harness::conformance::check(&said);
        assert!(
            said.iter()
                .any(|event| matches!(event, AgentEvent::Asked { .. })),
            "the ask was opened"
        );
        let declined = said.iter().any(|event| {
            matches!(
                event,
                AgentEvent::Work { item, .. } if item.status == ItemStatus::Declined
            )
        });
        assert!(declined, "the declined work row is there");
    }

    #[tokio::test]
    async fn a_turn_told_to_fail_fails() {
        let said = run("fail chunks=1 delay=0 tools=0", None).await;
        let broke = said
            .iter()
            .any(|event| matches!(event, AgentEvent::TurnDone { error: Some(_), .. }));
        assert!(broke);
    }
}
