//! What runs beside the main agent: the strip above the composer, and the workflow modal.
//!
//! The strip lists every live runner — subagents, workflows, background commands — so a thread
//! whose main agent idles still shows what it is doing. A workflow row opens the modal, which
//! lays the run out whole: its phases, the agents inside them, and what the script logged.

use zdt_agent::runner::{Runner, RunnerKind, WorkflowAgent};
use zdt_icons::{self as icons, IconProps};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::{component, view};

use crate::use_agent;
use zdt_view::Erase;

/// The live runners, one line each, above the composer.
#[component]
pub fn RunnerStrip() -> impl IntoView {
    let agent = use_agent();

    let ids = {
        let agent = agent.clone();
        move || {
            agent
                .client()
                .runners()
                .iter()
                .map(|runner| runner.id.clone())
                .collect::<Vec<String>>()
        }
    };

    view! {
        column(class = "agent-runners") {
            for id in move || ids(), key = |id: &String| id.clone() {
                RunnerLine(id = id)
            }
        }
    }
}

/// One runner's line. Reads the live set by id, so the same row repaints as its runner moves.
#[component]
fn RunnerLine(
    /// The runner's id in the watched thread's set.
    id: String,
) -> impl IntoView {
    let agent = use_agent();
    let held: RwSignal<Option<crate::AgentUi>, LocalStorage> =
        RwSignal::new_local(Some(agent.clone()));
    let key = RwSignal::new_local(id);

    let found = move || {
        held.with_untracked(Clone::clone).and_then(|agent| {
            key.with_untracked(|id| {
                agent
                    .client()
                    .runners()
                    .into_iter()
                    .find(|runner| &runner.id == id)
            })
        })
    };

    let glyph = {
        let found = found.clone();
        move || match found().map(|runner| runner.kind) {
            Some(RunnerKind::Workflow) => icons::WORKFLOW,
            Some(RunnerKind::Shell) => icons::TERMINAL,
            _ => icons::BOT,
        }
    };
    let title = {
        let found = found.clone();
        move || {
            found()
                .map(|runner| match runner.kind {
                    RunnerKind::Workflow => {
                        let name = runner
                            .workflow
                            .as_ref()
                            .map(|run| run.name.clone())
                            .unwrap_or_default();
                        if name.is_empty() {
                            runner.description
                        } else {
                            name
                        }
                    }
                    _ => runner.description,
                })
                .unwrap_or_default()
        }
    };
    let meta = {
        let found = found.clone();
        move || found().map(|runner| meta_text(&runner)).unwrap_or_default()
    };
    let is_workflow = {
        let found = found.clone();
        move || {
            matches!(
                found().map(|runner| runner.kind),
                Some(RunnerKind::Workflow)
            )
        }
    };
    let opens = {
        let is_workflow = is_workflow.clone();
        move || is_workflow().then(|| "true".to_owned())
    };
    let chevron_shown = move || (!is_workflow()).then(|| "none".to_owned());

    let press = {
        let found = found.clone();
        move |event: &mut EventCx<'_, events::PointerDown>| {
            event.stop_propagation();
            let (Some(agent), Some(runner)) = (held.with_untracked(Clone::clone), found()) else {
                return;
            };
            if runner.kind == RunnerKind::Workflow {
                agent.open_workflow(runner.id);
            }
        }
    };

    view! {
        row(class = "agent-runners__line", attr:data-opens = opens, on:pointer_down = press) {
            Icon(icon = icons::LOADER_CIRCLE, class = "icon--xs zdt-spin agent-runners__spin")
            Icon(icon = Signal::derive_local(glyph), class = "icon--xs")
            label(class = "agent-runners__title nowrap") {{title}}
            label(class = "agent-runners__meta muted nowrap") {{meta}}
            box(class = "fill") {}
            Icon(
                icon = icons::CHEVRON_RIGHT,
                class = "icon--xs agent-runners__go",
                style:display = chevron_shown
            )
        }
    }
}

/// One line of numbers and words under a runner: its type, what it spent, where it is.
fn meta_text(runner: &Runner) -> String {
    let mut parts: Vec<String> = Vec::new();
    if runner.kind == RunnerKind::Workflow {
        if let Some(run) = &runner.workflow {
            let live = run.agents.iter().filter(|agent| agent.is_live()).count();
            if live > 0 {
                parts.push(format!("{live} running"));
            }
        }
    } else if !runner.agent_type.is_empty() {
        parts.push(runner.agent_type.clone());
    }
    if runner.tokens > 0 {
        parts.push(tokens_text(runner.tokens));
    }
    if !runner.summary.is_empty() {
        parts.push(runner.summary.clone());
    } else if !runner.last_tool.is_empty() {
        parts.push(runner.last_tool.clone());
    }
    parts.join(" \u{00b7} ")
}

/// A token count in a few characters.
fn tokens_text(tokens: u64) -> String {
    if tokens >= 1000 {
        format!("{:.1}k tok", tokens as f64 / 1000.0)
    } else {
        format!("{tokens} tok")
    }
}

/// A duration in a few characters.
fn duration_text(ms: u64) -> String {
    let seconds = ms / 1000;
    if seconds >= 60 {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{seconds}s")
    }
}

/// The workflow modal. Mounted for good; an open runner id shows it.
///
/// `subject` is the card, registered as the modal's focus sink.
// The list macro takes a closure by construction, so the one it is handed here is not redundant.
#[allow(clippy::redundant_closure)]
#[component]
pub fn WorkflowModal(
    /// The card, for whoever gives it the keyboard.
    subject: NodeRef,
) -> impl IntoView {
    let agent = use_agent();
    // The portal's children are an `Fn` closure: everything they capture must be `Copy`, so
    // the state rides in signals and every view closure pulls it out.
    let held: RwSignal<Option<crate::AgentUi>, LocalStorage> =
        RwSignal::new_local(Some(agent.clone()));

    // The last picture of the opened runner. A workflow that drains from the live set keeps
    // its final picture on screen until the modal closes.
    let last: RwSignal<Option<Runner>, LocalStorage> = RwSignal::new_local(None);
    let ended: RwSignal<bool, LocalStorage> = RwSignal::new_local(false);

    let following = {
        let agent = agent.clone();
        zgui::reactive::RenderEffect::new(move |_| {
            let Some(id) = agent.workflow_open() else {
                last.set(None);
                ended.set(false);
                return;
            };
            match agent
                .client()
                .runners()
                .into_iter()
                .find(|runner| runner.id == id)
            {
                Some(runner) => {
                    last.set(Some(runner));
                    ended.set(false);
                }
                None => ended.set(last.with_untracked(Option::is_some)),
            }
        })
    };
    on_cleanup_local(move || drop(following));

    let shown = move || {
        held.with_untracked(|agent| agent.as_ref().map(crate::AgentUi::workflow_open))
            .flatten()
            .is_none()
            .then(|| "none".to_owned())
    };

    let close = move || {
        if let Some(agent) = held.with_untracked(Clone::clone) {
            agent.close_workflow();
        }
    };
    let on_key = move |cx: &mut EventCx<'_, events::KeyDown>| {
        use zgui::vocab::{Key, NamedKey};
        if matches!(&cx.key, Key::Named(NamedKey::Escape)) {
            close();
            cx.prevent_default();
            cx.stop_propagation();
        }
    };
    let taken = move |_: &mut EventCx<'_, events::FocusIn>| {
        if let Some(agent) = held.with_untracked(Clone::clone) {
            agent.host().took_keyboard();
        }
    };
    let dismiss = move |event: &mut EventCx<'_, events::PointerDown>| {
        event.stop_propagation();
        close();
    };
    let keep = move |event: &mut EventCx<'_, events::PointerDown>| {
        event.stop_propagation();
    };

    let title = move || {
        last.with(|held| {
            held.as_ref()
                .map(|runner| {
                    let name = runner
                        .workflow
                        .as_ref()
                        .map(|run| run.name.clone())
                        .unwrap_or_default();
                    if name.is_empty() {
                        runner.description.clone()
                    } else {
                        name
                    }
                })
                .unwrap_or_default()
        })
    };
    let standing = move || {
        if ended.get() {
            "finished".to_owned()
        } else {
            last.with(|held| {
                held.as_ref()
                    .map(|runner| meta_text(runner))
                    .unwrap_or_default()
            })
        }
    };
    let spinner_shown = move || ended.get().then(|| "none".to_owned());

    // The body: phases as sections, each with its agents, and the logs after.
    let body = move || {
        let Some(runner) = last.get() else {
            return ().any();
        };
        let Some(run) = runner.workflow else {
            return ().any();
        };
        // Agents sit under the phase that names them; a phase nobody announced still shows.
        let mut phases: Vec<String> = run.phases.clone();
        for agent in &run.agents {
            if !agent.phase.is_empty() && !phases.contains(&agent.phase) {
                phases.push(agent.phase.clone());
            }
        }
        let mut parts: Vec<zgui::view::AnyView> = Vec::new();
        for phase in phases {
            let crew: Vec<WorkflowAgent> = run
                .agents
                .iter()
                .filter(|agent| agent.phase == phase)
                .cloned()
                .collect();
            let done = crew.iter().filter(|agent| !agent.is_live()).count();
            let count = crew.len();
            let tally = if count > 0 {
                format!("{done}/{count}")
            } else {
                String::new()
            };
            parts.push(
                view! {
                    row(class = "agent-workflow__phase") {
                        label(class = "nowrap") {{phase}}
                        box(class = "fill") {}
                        label(class = "muted nowrap") {{tally}}
                    }
                }
                .any(),
            );
            for agent in crew {
                parts.push(agent_row(agent));
            }
        }
        if !run.logs.is_empty() {
            parts.push(
                view! {
                    row(class = "agent-workflow__phase") {
                        label(class = "nowrap") {"Log"}
                    }
                }
                .any(),
            );
            for line in run.logs {
                parts.push(
                    view! {
                        label(class = "agent-workflow__log muted") {{line}}
                    }
                    .any(),
                );
            }
        }
        parts.any()
    };

    view! {
        Portal {
            box(class = "agent-workflow__backdrop", style:display = shown, on:pointer_down = dismiss) {
                column(
                    class = "agent-workflow",
                    node_ref = subject,
                    tabindex = Focus::Programmatic,
                    on:pointer_down = keep,
                    on:key_down = on_key,
                    on:focus_in = taken
                ) {
                    row(class = "agent-workflow__head") {
                        Icon(icon = icons::WORKFLOW, class = "icon--sm")
                        label(class = "agent-workflow__title nowrap") {{title}}
                        box(class = "fill") {}
                        Icon(
                            icon = icons::LOADER_CIRCLE,
                            class = "icon--xs zdt-spin agent-workflow__spin",
                            style:display = spinner_shown
                        )
                        label(class = "muted nowrap") {{standing}}
                    }
                    scroll(class = "agent-workflow__body") {
                        {body}
                    }
                }
            }
        }
    }
}

/// One agent's row inside the modal: who it is on the left, what it has spent on the right.
fn agent_row(agent: WorkflowAgent) -> zgui::view::AnyView {
    let (glyph, tone) = match agent.state.as_str() {
        "done" => (icons::CIRCLE_CHECK, "done"),
        "error" => (icons::CIRCLE_ALERT, "error"),
        "start" => (icons::CIRCLE_DASHED, "start"),
        _ => (icons::LOADER_CIRCLE, "progress"),
    };
    let mut spent: Vec<String> = Vec::new();
    if agent.tokens > 0 {
        spent.push(tokens_text(agent.tokens));
    }
    if agent.tool_calls > 0 {
        spent.push(format!("{} tools", agent.tool_calls));
    }
    if agent.duration_ms > 0 {
        spent.push(duration_text(agent.duration_ms));
    }
    let live = matches!(agent.state.as_str(), "progress");
    let mark = if live {
        "icon--xs zdt-spin"
    } else {
        "icon--xs"
    };
    view! {
        row(class = "agent-workflow__agent", attr:data-state = Some(tone.to_owned())) {
            Icon(icon = glyph, class = mark)
            label(class = "agent-workflow__label nowrap") {{agent.label}}
            label(class = "muted nowrap") {{agent.model}}
            box(class = "fill") {}
            label(class = "agent-workflow__spent muted nowrap") {{spent.join(" \u{00b7} ")}}
        }
    }
    .any()
}
