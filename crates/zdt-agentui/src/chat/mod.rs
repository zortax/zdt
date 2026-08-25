//! The chat view: one thread's whole conversation.

mod ask;
pub mod commit;
mod composer;
mod plan;
pub mod review;
pub mod runners;
pub mod timeline;

pub use crate::chat::commit::{CommitModal, CommitModalProps};
pub use crate::chat::composer::{Composer, ComposerProps};
pub use crate::chat::review::{ReviewPane, ReviewPaneProps};
pub use crate::chat::runners::{WorkflowModal, WorkflowModalProps};
pub use crate::chat::timeline::{Timeline, TimelineProps};

use zdt_agent::thread::{ThreadState, Usage};
use zdt_icons::{self as icons, IconProps};
use zgui::prelude::*;
use zgui::{component, view};

use crate::chat::ask::AskCardProps;
use crate::chat::plan::{PlanCardProps, TodoStripProps};
use crate::chat::runners::RunnerStripProps;
use crate::use_agent;

/// The whole view: a header, the timeline, and the composer's place.
///
/// `composer` is the composer's own editor element, registered as the composer's focus sink;
/// `chat` is the timeline's node, registered as the chat's. `controls` is what the editor puts
/// at the header's far end — the window buttons, in the same corner they hold in editor mode.
#[component]
pub fn AgentView(
    /// The composer's editor element, for whoever gives it the keyboard.
    composer: NodeRef,
    /// The timeline's node, for whoever gives it the keyboard.
    chat: NodeRef,
    /// The review surface's node, for whoever gives it the keyboard.
    review: NodeRef,
    /// What sits at the header's far end.
    controls: zgui::view::AnyView,
) -> impl IntoView {
    let agent = use_agent();
    let window = use_window();

    let showing = {
        let agent = agent.clone();
        move || (agent.screen() == crate::Screen::Agent).then(|| "true".to_owned())
    };
    let shell = {
        let agent = agent.clone();
        move || agent.selected_shell()
    };
    let title = {
        let shell = shell.clone();
        move || shell().map(|shell| shell.title).unwrap_or_default()
    };
    let project = {
        let shell = shell.clone();
        move || shell().map(|shell| shell.project).unwrap_or_default()
    };
    let state = {
        let shell = shell.clone();
        move || shell().map(|shell| shell.state).unwrap_or_default()
    };
    let failed = {
        let shell = shell.clone();
        move || {
            shell()
                .filter(|shell| shell.state == ThreadState::Failed)
                .and_then(|shell| shell.last_error)
                .unwrap_or_default()
        }
    };
    let has_failed = {
        let state = state.clone();
        move || (state() != ThreadState::Failed).then(|| "none".to_owned())
    };
    let weight = {
        let shell = shell.clone();
        move || {
            shell()
                .map(|shell| usage_text(shell.usage))
                .unwrap_or_default()
        }
    };

    // The timeline steps aside while a review is open; both stay mounted.
    let timeline_shown = {
        let agent = agent.clone();
        move || agent.review().is_some().then(|| "none".to_owned())
    };

    // The checkout under the thread moved off its branch: worth a standing line.
    let mismatch = {
        let shell = shell.clone();
        move || {
            shell()
                .filter(|shell| {
                    !shell.branch.is_empty()
                        && !shell.on_branch.is_empty()
                        && shell.on_branch != shell.branch
                })
                .map(|shell| {
                    format!(
                        "the checkout is on {}; the thread expects {}",
                        shell.on_branch, shell.branch
                    )
                })
                .unwrap_or_default()
        }
    };
    let mismatch_shown = {
        let mismatch = mismatch.clone();
        move || mismatch().is_empty().then(|| "none".to_owned())
    };

    // Whether an ask stands where the composer was.
    let asked = {
        let agent = agent.clone();
        move || agent.asking().is_some()
    };
    let composer_shown = {
        let asked = asked.clone();
        move || asked().then(|| "none".to_owned())
    };

    // An ask takes the keyboard out of the composer: the decision keys live in normal mode. New
    // asks also drop any half-taken answers.
    let steering = {
        let agent = agent.clone();
        zgui::reactive::RenderEffect::new(move |_| {
            let asking = agent.asking();
            agent.clear_answers();
            if asking.is_some()
                && agent.screen() == crate::Screen::Agent
                && agent.wants() == crate::Want::Composer
            {
                agent.to_chat();
            }
        })
    };
    on_cleanup_local(move || drop(steering));

    // The header's git actions: review everything, commit what stands.
    let open_review = {
        let agent = agent.clone();
        move |event: &mut EventCx<'_, events::PointerDown>| {
            event.stop_propagation();
            agent.review_thread();
        }
    };
    let start_commit = {
        let agent = agent.clone();
        move |event: &mut EventCx<'_, events::PointerDown>| {
            event.stop_propagation();
            agent.open_commit(false);
        }
    };

    // The provider's mark before the title: which agent this conversation is with. Filled art
    // and the stroked fallback are separate elements, as everywhere.
    let head_mark = {
        let agent = agent.clone();
        move || agent.provider_mark()
    };
    let head_mark_icon = {
        let head_mark = head_mark.clone();
        move || head_mark().unwrap_or(icons::DOT)
    };
    let head_mark_shown = {
        let head_mark = head_mark.clone();
        move || head_mark().is_none().then(|| "none".to_owned())
    };
    let head_fallback_shown = move || head_mark().is_some().then(|| "none".to_owned());

    view! {
        column(class = "agent-view", attr:data-open = showing) {
            // The header is the title strip: a press anywhere in it that is not on a control
            // drags the window, exactly as the editor's own header does.
            row(class = "agent-view__head", on:pointer_down = window.move_drag_handler()) {
                Icon(
                    icon = Signal::derive_local(head_mark_icon),
                    class = "icon--sm icon--brand",
                    style:display = head_mark_shown
                )
                Icon(icon = icons::BOT, class = "icon--sm", style:display = head_fallback_shown)
                label(class = "agent-view__title nowrap") {{title}}
                label(class = "agent-view__project muted nowrap") {{project}}
                box(class = "fill") {}
                label(class = "agent-view__usage muted nowrap") {{weight}}
                control(
                    class = "agent-view__tool",
                    tabindex = Focus::Programmatic,
                    a11y:label = "Review every change",
                    on:pointer_down = open_review
                ) {
                    Icon(icon = icons::FILE_DIFF, class = "icon--xs")
                    label(class = "nowrap") {"review"}
                }
                control(
                    class = "agent-view__tool",
                    tabindex = Focus::Programmatic,
                    a11y:label = "Commit the working tree",
                    on:pointer_down = start_commit
                ) {
                    Icon(icon = icons::GIT_COMMIT, class = "icon--xs")
                    label(class = "nowrap") {"commit"}
                }
                {controls}
            }
            row(class = "agent-view__error", style:display = has_failed) {
                Icon(icon = icons::CIRCLE_ALERT, class = "icon--xs")
                label(class = "nowrap") {{failed}}
            }
            box(class = "agent-view__log", style:display = timeline_shown) {
                Timeline(node = chat)
            }
            ReviewPane(node = review)
            column(class = "agent-view__foot") {
                TodoStrip()
                row(class = "agent-view__banner agent-view__banner--warn", style:display = mismatch_shown) {
                    Icon(icon = icons::GIT_BRANCH, class = "icon--xs")
                    label(class = "muted") {{mismatch}}
                }
                RunnerStrip()
                PlanCard()
                AskCard()
                box(style:display = composer_shown) {
                    Composer(field = composer)
                }
            }
        }
    }
}

/// What the conversation weighs, in a few characters.
fn usage_text(usage: Usage) -> String {
    if usage.context_tokens == 0 {
        return String::new();
    }
    let tokens = if usage.context_tokens >= 1000 {
        format!("{:.1}k", usage.context_tokens as f64 / 1000.0)
    } else {
        usage.context_tokens.to_string()
    };
    let mut said = tokens;
    if usage.context_limit > 0 {
        let percent = (usage.context_tokens as f64 / usage.context_limit as f64) * 100.0;
        said.push_str(&format!(" \u{00b7} {percent:.0}%"));
    }
    if usage.cost_usd > 0.0 {
        said.push_str(&format!(" \u{00b7} ${:.2}", usage.cost_usd));
    }
    said
}
