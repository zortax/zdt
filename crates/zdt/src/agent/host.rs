//! This editor, as the agent surface sees it.
//!
//! Everything here reads its session out of the local context at call time: the surface is one,
//! the sessions are many, and a verb always means the session whose subtree the call came from.
//! The one exception is opening a project, which is the registry's business.

use std::path::Path;

use zgui::vocab::{KeyEvent, Modifiers};

use crate::session::SessionKey;
use crate::session::host::SessionHost;

/// The editor, as the surface sees it.
pub struct Editor {
    sessions: SessionHost,
}

impl Editor {
    /// A host over the session registry.
    #[must_use]
    pub fn new(sessions: SessionHost) -> Self {
        Self { sessions }
    }

    /// The session the calling subtree draws, when there is one.
    fn session(&self) -> Option<crate::session::Session> {
        crate::session::use_session()
    }
}

impl zdt_agentui::Host for Editor {
    fn say(&self, said: &str) {
        match self.session() {
            Some(session) => session.workspace().say(said),
            None => tracing::info!("agent: {said}"),
        }
    }

    fn complain(&self, said: &str) {
        match self.session() {
            Some(session) => session.announcer().fail("agent", Some(said.to_owned())),
            None => tracing::warn!("agent: {said}"),
        }
    }

    fn open_project(&self, root: &Path, inherits: Option<&Path>) {
        let Some(key) = SessionKey::of(root) else {
            self.complain(&format!("{} is not a directory", root.display()));
            return;
        };
        // A worktree session that was never opened starts from its project's saved state: the
        // base is written now, so the clone carries what is on screen, and the restore aims
        // every path at the worktree's checkout.
        if let Some(base) = inherits
            && self.sessions.find(&key).is_none()
            && let Some(state) = zdt_core::state::State::discover()
        {
            if let Some(open) = SessionKey::of(base)
                .and_then(|held| self.sessions.find(&held))
                .and_then(|id| self.sessions.session(id))
            {
                open.flush();
            }
            let target = key.path().unwrap_or(root);
            let base = std::fs::canonicalize(base).unwrap_or_else(|_| base.to_path_buf());
            crate::session::store::clone_into(&state, &base, target);
        }
        self.sessions.reveal(key.clone(), &[]);
        // The keyboard follows onto the revealed session's own surface.
        if let Some(session) = self
            .sessions
            .find(&key)
            .and_then(|id| self.sessions.session(id))
        {
            session.workspace().focus().enter_agent();
        }
    }

    fn open_file(&self, path: &Path, line: Option<u64>) {
        let Some(session) = self.session() else {
            return;
        };
        crate::files::open_at(session.workspace(), path, line);
        // Reading a diff hands over to reading the file: the keyboard goes to the panes, and
        // the window turns to the editor when the chat was on screen.
        if let Some(agent) = zdt_agentui::try_use_agent()
            && agent.screen() == zdt_agentui::Screen::Agent
        {
            agent.toggle_screen();
        }
    }

    fn ask_line(&self, title: &str, start: &str, then: std::rc::Rc<dyn Fn(String)>) {
        let Some(prompt) = zgui::reactive::use_local_context::<crate::prompt::Prompt>() else {
            tracing::warn!("agent: no prompt to ask with");
            return;
        };
        prompt.ask(title.to_owned(), start.to_owned(), move |typed: &str| {
            then(typed.to_owned());
        });
    }

    fn focus_agent(&self) {
        if let Some(session) = self.session() {
            session.workspace().focus().enter_agent();
        }
    }

    fn leave(&self) {
        if let Some(session) = self.session() {
            session.workspace().focus().enter_panes();
        }
    }

    fn took_keyboard(&self) {
        self.focus_agent();
    }

    fn key(&self, event: &KeyEvent, modifiers: Modifiers, region: &'static str) -> bool {
        let Some(session) = self.session() else {
            return false;
        };
        crate::keys::chord_of(event, modifiers)
            .is_some_and(|chord| session.vim().key_in_region(chord, region))
    }

    fn has_keyboard(&self) -> bool {
        crate::focus::try_use_focus().is_some_and(|focus| focus.in_agent())
    }

    fn files(&self, then: std::rc::Rc<dyn Fn(Vec<String>)>) {
        let Some(session) = self.session() else {
            return;
        };
        let root = session.workspace().project().root().to_path_buf();
        zdt_view::detached(async move {
            let walked = zgui::task::blocking(move || {
                zdt_core::search::files::walk(&root, zdt_core::search::files::Walk::default())
            })
            .await;
            then(walked);
        });
    }

    fn models(&self) -> Vec<String> {
        crate::settings::use_settings().with_untracked(|config| config.agent.models.clone())
    }

    fn instances(&self) -> Vec<(String, String)> {
        crate::settings::use_settings().with_untracked(|config| {
            let agent = &config.agent;
            // An empty table means the daemon's two synthesized instances.
            let mut rows: Vec<(String, String)> = if agent.instances.is_empty() {
                vec![
                    ("claude".to_owned(), "claude".to_owned()),
                    ("codex".to_owned(), "codex".to_owned()),
                ]
            } else {
                agent
                    .instances
                    .iter()
                    .map(|(name, instance)| {
                        let provider = if instance.provider.is_empty() {
                            name.clone()
                        } else {
                            instance.provider.clone()
                        };
                        (name.clone(), provider)
                    })
                    .collect()
            };
            // The default first, so a picker's top row is what an empty choice means.
            let default = if rows.iter().any(|(name, _)| *name == agent.instance) {
                agent.instance.clone()
            } else {
                "claude".to_owned()
            };
            rows.sort_by_key(|(name, _)| *name != default);
            rows
        })
    }

    fn project_root(&self) -> Option<std::path::PathBuf> {
        self.session()
            .map(|session| session.workspace().project().root().to_path_buf())
    }

    fn offer(&self) -> Option<zdt_agentui::Offer> {
        use crate::picker::{Deed, Picker, Row, Source, Target};
        // Resolved here, where the caller's context still answers; the hand it becomes works
        // from anywhere later.
        let picker = zgui::reactive::use_local_context::<Picker>()?;
        Some(std::rc::Rc::new(
            move |title: &'static str,
                  rows: Vec<(String, String)>,
                  then: std::rc::Rc<dyn Fn(usize)>| {
                let rows = rows
                    .into_iter()
                    .enumerate()
                    .map(|(index, (label, detail))| {
                        let then = std::rc::Rc::clone(&then);
                        Row::plain(label, Target::Run(Deed::new(move || then(index))))
                            .with_detail(detail)
                    })
                    .collect();
                picker.open(Source::Given {
                    title,
                    rows,
                    typed: None,
                });
            },
        ))
    }

    fn streams_text(&self) -> bool {
        // Tracked, so flipping `[agent] stream` in the configuration takes effect in place.
        crate::settings::use_settings().with(|config| config.agent.stream)
    }
}
