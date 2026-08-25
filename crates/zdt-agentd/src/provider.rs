//! The provider instances the daemon drives.
//!
//! One instance is one configured account of one harness: a name, an adapter, and the model its
//! threads default to. The registry is built from the configuration once at start; a thread
//! carries its instance's name, and every command finds the adapter through it.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use tokio::sync::mpsc::UnboundedSender;
use zdt_agent::ask::Decision;
use zdt_agent::event::AgentEvent;
use zdt_agent::mode::RuntimeMode;
use zdt_agent::thread::ThreadId;
use zdt_agent_claude::ClaudeAdapter;
use zdt_agent_codex::CodexAdapter;
use zdt_agent_harness::{HarnessError, ProviderAdapter, SessionStart};

/// One adapter, whichever harness it is.
///
/// Cloning one is cloning a handle onto the same sessions.
#[derive(Clone)]
pub enum Provider {
    /// Claude Code over stream-json.
    Claude(ClaudeAdapter),
    /// Codex over its app server.
    Codex(CodexAdapter),
    /// The in-process mock, for load and layout work that must cost nothing.
    Mock(crate::mock::MockAdapter),
}

impl Provider {
    /// Which harness this is: the word the sidebar's glyph goes by.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Claude(adapter) => adapter.kind(),
            Self::Codex(adapter) => adapter.kind(),
            Self::Mock(adapter) => adapter.kind(),
        }
    }

    /// Learns what a session in `cwd` would offer, without running one for real.
    pub fn probe(&self, thread: ThreadId, cwd: PathBuf) {
        match self {
            Self::Claude(adapter) => adapter.probe(thread, cwd),
            Self::Codex(adapter) => adapter.probe(thread, cwd),
            Self::Mock(adapter) => adapter.probe(thread),
        }
    }

    /// Sends a prompt, starting or resuming the thread's session first when it has to.
    pub async fn send_turn(&self, start: SessionStart, text: String) -> Result<(), HarnessError> {
        match self {
            Self::Claude(adapter) => adapter.send_turn(start, text).await,
            Self::Codex(adapter) => adapter.send_turn(start, text).await,
            Self::Mock(adapter) => adapter.send_turn(start, text).await,
        }
    }

    /// Stops the turn that is running.
    pub async fn interrupt(&self, thread: ThreadId) -> Result<(), HarnessError> {
        match self {
            Self::Claude(adapter) => adapter.interrupt(thread).await,
            Self::Codex(adapter) => adapter.interrupt(thread).await,
            Self::Mock(adapter) => adapter.interrupt(thread).await,
        }
    }

    /// Answers an open tool ask.
    pub async fn decide(
        &self,
        thread: ThreadId,
        id: String,
        decision: Decision,
    ) -> Result<(), HarnessError> {
        match self {
            Self::Claude(adapter) => adapter.decide(thread, id, decision).await,
            Self::Codex(adapter) => adapter.decide(thread, id, decision).await,
            Self::Mock(adapter) => adapter.decide(thread, id, decision).await,
        }
    }

    /// Answers an open question ask.
    pub async fn answer(
        &self,
        thread: ThreadId,
        id: String,
        answers: Vec<Vec<String>>,
    ) -> Result<(), HarnessError> {
        match self {
            Self::Claude(adapter) => adapter.answer(thread, id, answers).await,
            Self::Codex(adapter) => adapter.answer(thread, id, answers).await,
            Self::Mock(adapter) => adapter.answer(thread, id, answers).await,
        }
    }

    /// Moves a live session to `mode`.
    pub async fn set_mode(&self, thread: ThreadId, mode: RuntimeMode) -> Result<(), HarnessError> {
        match self {
            Self::Claude(adapter) => adapter.set_mode(thread, mode).await,
            Self::Codex(adapter) => adapter.set_mode(thread, mode).await,
            Self::Mock(adapter) => adapter.set_mode(thread, mode).await,
        }
    }

    /// Moves a live session to `model`.
    pub async fn set_model(&self, thread: ThreadId, model: String) -> Result<(), HarnessError> {
        match self {
            Self::Claude(adapter) => adapter.set_model(thread, model).await,
            Self::Codex(adapter) => adapter.set_model(thread, model).await,
            Self::Mock(adapter) => adapter.set_model(thread, model).await,
        }
    }

    /// Stops the thread's session.
    pub async fn stop(&self, thread: ThreadId) {
        match self {
            Self::Claude(adapter) => adapter.stop(thread).await,
            Self::Codex(adapter) => adapter.stop(thread).await,
            Self::Mock(adapter) => adapter.stop(thread).await,
        }
    }

    /// Stops every session.
    pub async fn stop_all(&self) {
        match self {
            Self::Claude(adapter) => adapter.stop_all().await,
            Self::Codex(adapter) => adapter.stop_all().await,
            Self::Mock(adapter) => adapter.stop_all().await,
        }
    }

    /// Answers one prompt outside any session, as plain text.
    ///
    /// What every drafting job runs on. `model` empty lets the harness pick its own cheap word.
    /// Nothing when the harness cannot say. The mock answers a fixed commit draft, so drafting
    /// can be exercised without an agent behind it.
    pub async fn generate(&self, model: &str, prompt: &str) -> Option<String> {
        match self {
            Self::Claude(adapter) => adapter.oneshot(model, prompt).await,
            Self::Codex(adapter) => adapter.oneshot(model, prompt).await,
            Self::Mock(_) => Some(
                r#"{"subject":"Mock the pending change","body":"- everything the mock saw","branch":"feat/mock-pending-change"}"#
                    .to_owned(),
            ),
        }
    }

    /// The conversations the harness already holds on disk, offered for import. Blocking.
    #[must_use]
    pub fn importable(&self) -> Vec<zdt_agent_harness::FoundImport> {
        match self {
            Self::Claude(adapter) => adapter.importable(),
            Self::Codex(adapter) => adapter.importable(),
            Self::Mock(_) => Vec::new(),
        }
    }

    /// One of them, read whole. Blocking.
    #[must_use]
    pub fn import_dump(&self, id: &str) -> Option<zdt_agent_harness::SessionDump> {
        match self {
            Self::Claude(adapter) => adapter.import_dump(id),
            Self::Codex(adapter) => adapter.import_dump(id),
            Self::Mock(_) => None,
        }
    }

    /// Makes up a short title for a thread that started with `prompt`.
    ///
    /// One cheap request outside any session; `model` empty lets the harness pick its own cheap
    /// word. Nothing when the harness cannot say.
    pub async fn title(&self, model: &str, prompt: &str) -> Option<String> {
        let clipped: String = prompt.chars().take(2000).collect();
        let asked = format!(
            "Name this coding task in at most six words. Answer with the name only: no quotes, \
             no trailing period.\n\nTask:\n{clipped}",
        );
        match self {
            Self::Claude(adapter) => adapter.oneshot(model, &asked).await,
            Self::Codex(adapter) => adapter.oneshot(model, &asked).await,
            Self::Mock(_) => Some(crate::mock::MockAdapter::title_of(prompt)),
        }
    }
}

/// One registered instance.
struct Registered {
    adapter: Provider,
    /// The model its threads talk to when they name none.
    model: String,
}

/// Every configured instance, by name.
pub struct Providers {
    map: HashMap<String, Registered>,
    /// The instance new threads run on when none is chosen.
    default_name: String,
}

impl Providers {
    /// The registry the configuration describes.
    ///
    /// An empty instance table means one `claude` and one `codex` instance with everything
    /// default. An instance naming a harness this build has no word for is skipped with a
    /// warning, and its threads are refused until a build that knows it.
    #[must_use]
    pub fn from_config(
        config: &zdt_core::config::Config,
        events: &UnboundedSender<AgentEvent>,
        logs: Option<&Path>,
    ) -> Self {
        let agent = &config.agent;
        let mut map = HashMap::new();

        if agent.instances.is_empty() {
            map.insert(
                "claude".to_owned(),
                Registered {
                    adapter: Provider::Claude(ClaudeAdapter::new(
                        events.clone(),
                        agent.binary.clone(),
                        String::new(),
                        logs.map(Path::to_path_buf),
                    )),
                    model: agent.model.clone(),
                },
            );
            map.insert(
                "codex".to_owned(),
                Registered {
                    adapter: Provider::Codex(CodexAdapter::new(
                        events.clone(),
                        String::new(),
                        String::new(),
                        logs.map(Path::to_path_buf),
                    )),
                    model: String::new(),
                },
            );
        }
        for (name, instance) in &agent.instances {
            let kind = if instance.provider.is_empty() {
                name.as_str()
            } else {
                instance.provider.as_str()
            };
            let adapter = match kind {
                "claude" => Provider::Claude(ClaudeAdapter::new(
                    events.clone(),
                    instance.binary.clone(),
                    instance.home.clone(),
                    logs.map(Path::to_path_buf),
                )),
                "codex" => Provider::Codex(CodexAdapter::new(
                    events.clone(),
                    instance.binary.clone(),
                    instance.home.clone(),
                    logs.map(Path::to_path_buf),
                )),
                // Never synthesized: only an instance that names it gets the mock.
                "mock" => Provider::Mock(crate::mock::MockAdapter::new(events.clone())),
                other => {
                    tracing::warn!("instance {name}: no harness called {other}; skipped");
                    continue;
                }
            };
            map.insert(
                name.clone(),
                Registered {
                    adapter,
                    model: instance.model.clone(),
                },
            );
        }

        let default_name = if map.contains_key(&agent.instance) {
            agent.instance.clone()
        } else if map.contains_key("claude") {
            "claude".to_owned()
        } else {
            map.keys().min().cloned().unwrap_or_default()
        };
        Self { map, default_name }
    }

    /// The instance under `name`; the default when `name` is empty.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Provider> {
        let name = if name.is_empty() {
            &self.default_name
        } else {
            name
        };
        self.map.get(name).map(|held| &held.adapter)
    }

    /// The name an empty choice means.
    #[must_use]
    pub fn default_name(&self) -> &str {
        &self.default_name
    }

    /// The model `name`'s threads default to.
    #[must_use]
    pub fn default_model(&self, name: &str) -> &str {
        let name = if name.is_empty() {
            &self.default_name
        } else {
            name
        };
        self.map.get(name).map_or("", |held| held.model.as_str())
    }

    /// Stops every session of every instance.
    pub async fn stop_all(&self) {
        for held in self.map.values() {
            held.adapter.stop_all().await;
        }
    }

    /// The instance drafting jobs run on: `wanted` when it names one, then a codex instance,
    /// then a claude one, then the default.
    #[must_use]
    pub fn messenger(&self, wanted: &str) -> Option<&Provider> {
        if !wanted.is_empty()
            && let Some(held) = self.map.get(wanted)
        {
            return Some(&held.adapter);
        }
        for kind in ["codex", "claude"] {
            let found = self
                .map
                .iter()
                .filter(|(_, held)| held.adapter.kind() == kind)
                .map(|(name, held)| (name, &held.adapter))
                .min_by_key(|(name, _)| (*name != &self.default_name, (*name).clone()));
            if let Some((_, adapter)) = found {
                return Some(adapter);
            }
        }
        self.get("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel() -> UnboundedSender<AgentEvent> {
        tokio::sync::mpsc::unbounded_channel().0
    }

    #[tokio::test]
    async fn an_empty_table_means_one_claude_and_one_codex() {
        let config = zdt_core::config::Config::default();
        let providers = Providers::from_config(&config, &channel(), None);
        assert_eq!(providers.get("claude").map(Provider::kind), Some("claude"));
        assert_eq!(providers.get("codex").map(Provider::kind), Some("codex"));
        assert_eq!(providers.default_name(), "claude");
        assert_eq!(providers.get("").map(Provider::kind), Some("claude"));
    }

    #[tokio::test]
    async fn two_homes_are_two_instances_of_one_harness() {
        let mut config = zdt_core::config::Config::default();
        for (name, home) in [("work", "/tmp/a"), ("personal", "/tmp/b")] {
            config.agent.instances.insert(
                name.to_owned(),
                zdt_core::config::Instance {
                    provider: "claude".to_owned(),
                    home: home.to_owned(),
                    ..zdt_core::config::Instance::default()
                },
            );
        }
        config.agent.instance = "personal".to_owned();
        let providers = Providers::from_config(&config, &channel(), None);
        assert_eq!(providers.get("work").map(Provider::kind), Some("claude"));
        assert_eq!(
            providers.get("personal").map(Provider::kind),
            Some("claude")
        );
        assert_eq!(providers.default_name(), "personal");
    }

    #[tokio::test]
    async fn an_unknown_harness_is_skipped_and_a_named_instance_keeps_its_model() {
        let mut config = zdt_core::config::Config::default();
        config.agent.instances.insert(
            "cursor".to_owned(),
            zdt_core::config::Instance {
                provider: "cursor".to_owned(),
                ..zdt_core::config::Instance::default()
            },
        );
        config.agent.instances.insert(
            "codex".to_owned(),
            zdt_core::config::Instance {
                model: "gpt-5.6-luna".to_owned(),
                ..zdt_core::config::Instance::default()
            },
        );
        let providers = Providers::from_config(&config, &channel(), None);
        assert!(providers.get("cursor").is_none());
        assert_eq!(providers.get("codex").map(Provider::kind), Some("codex"));
        assert_eq!(providers.default_model("codex"), "gpt-5.6-luna");
    }
}
