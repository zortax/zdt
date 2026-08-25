//! The decision surface.
//!
//! When a turn stops to ask, the composer's place becomes the question: what the tool would do,
//! or which option to take. Everything answers to single keys in the chat's normal mode, and the
//! card says which.

use zdt_agent::ask::{Ask, AskKind};
use zdt_icons::{self as icons, IconProps};
use zgui::prelude::*;
use zgui::{component, view};

use crate::use_agent;

/// The card standing where the composer was.
#[component]
pub fn AskCard() -> impl IntoView {
    use zdt_view::Erase;

    let agent = use_agent();

    let count = {
        let agent = agent.clone();
        move || agent.client().asks().len()
    };
    let card = move || match agent.asking() {
        None => ().any(),
        Some(ask) => {
            let more = count().saturating_sub(1);
            view! { OneAsk(ask = ask, more = more) }.any()
        }
    };

    view! {
        {card}
    }
}

/// One ask, whole.
#[component]
fn OneAsk(
    /// What is asked.
    ask: Ask,
    /// How many more wait behind it.
    more: usize,
) -> impl IntoView {
    use zdt_view::Erase;

    let queue = (more > 0).then(|| format!("+{more} more"));
    let body = match ask.kind {
        AskKind::Tool {
            name,
            tool,
            summary,
            detail,
        } => {
            let glyph = crate::chat::timeline::tool_glyph_for(tool);
            let same = detail == summary || detail.is_empty();
            let keys = view! {
                row(class = "agent-ask__keys") {
                    Key(key = "a", what = "approve")
                    Key(key = "A", what = "always")
                    Key(key = "d", what = "deny")
                }
            };
            view! {
                column(class = "agent-ask__body") {
                    row(class = "agent-ask__head") {
                        Icon(icon = glyph, class = "icon--sm agent-ask__glyph")
                        label(class = "agent-ask__name") {{name}}
                        label(class = "agent-ask__summary muted nowrap") {{summary}}
                    }
                    {
                        if same {
                            ().any()
                        } else {
                            view! {
                                scroll(class = "agent-ask__detail") {
                                    label(class = "agent-ask__mono") {{detail}}
                                }
                            }
                            .any()
                        }
                    }
                    {keys}
                }
            }
            .any()
        }
        AskKind::Question { questions } => view! { AskQuestions(questions = questions) }.any(),
        AskKind::Unknown => view! {
            column(class = "agent-ask__body") {
                label(class = "muted") {"The agent asked something this build has no words for."}
                row(class = "agent-ask__keys") {
                    Key(key = "d", what = "decline")
                }
            }
        }
        .any(),
    };

    view! {
        column(class = "agent-ask") {
            row(class = "agent-ask__title") {
                Icon(icon = icons::CIRCLE_QUESTION, class = "icon--sm")
                label {"Waiting on you"}
                box(class = "fill") {}
                label(class = "muted") {{queue.unwrap_or_default()}}
            }
            {body}
        }
    }
}

/// The questions, one at a time.
#[component]
fn AskQuestions(
    /// The questions, answered in order.
    questions: Vec<zdt_agent::ask::Question>,
) -> impl IntoView {
    use zdt_view::Erase;

    let agent = use_agent();
    let held = std::rc::Rc::new(questions);

    let body = {
        let agent = agent.clone();
        move || {
            let at = agent.question_at();
            let Some(question) = held.get(at).cloned() else {
                return ().any();
            };
            let taken = agent.toggled();
            let counter = if held.len() > 1 {
                format!("{} of {}", at + 1, held.len())
            } else {
                String::new()
            };
            let hint = if question.multi {
                "number toggles \u{00b7} \u{23ce} confirm"
            } else {
                "number answers"
            };
            let options: Vec<OptionFacts> = question
                .options
                .iter()
                .enumerate()
                .map(|(index, option)| OptionFacts {
                    number: index + 1,
                    label: option.label.clone(),
                    description: option.description.clone(),
                    on: question.multi && taken.contains(&option.label),
                })
                .collect();
            view! {
                column(class = "agent-ask__body") {
                    row(class = "agent-ask__head") {
                        label(class = "agent-ask__name") {{question.header.clone()}}
                        label(class = "muted") {{counter}}
                    }
                    label(class = "agent-ask__question") {{question.question.clone()}}
                    column(class = "agent-ask__options") {
                        for option in move || options.clone(), key = |option: &OptionFacts| option.number {
                            OptionRow(option = option)
                        }
                    }
                    row(class = "agent-ask__keys") {
                        label(class = "muted") {{hint}}
                    }
                }
            }
            .any()
        }
    };

    view! {
        {body}
    }
}

/// One option, as the card draws it.
#[derive(Clone, PartialEq)]
struct OptionFacts {
    number: usize,
    label: String,
    description: String,
    on: bool,
}

/// One option row.
#[component]
fn OptionRow(
    /// The option.
    option: OptionFacts,
) -> impl IntoView {
    let agent = use_agent();

    let OptionFacts {
        number,
        label: label_text,
        description,
        on,
    } = option;
    let pick = move |event: &mut EventCx<'_, events::PointerDown>| {
        event.stop_propagation();
        agent.choose(number - 1);
    };
    let lit = move || on.then(|| "true".to_owned());

    view! {
        row(
            class = "agent-ask__option",
            attr:data-on = lit,
            on:pointer_down = pick
        ) {
            label(class = "agent-ask__number") {{format!("{number}")}}
            label {{label_text}}
            label(class = "muted nowrap") {{description}}
        }
    }
}

/// One key and what it does.
#[component]
fn Key(
    /// The key.
    key: &'static str,
    /// What pressing it does.
    what: &'static str,
) -> impl IntoView {
    view! {
        row(class = "agent-ask__key") {
            label(class = "agent-ask__cap") {{key}}
            label(class = "muted") {{what}}
        }
    }
}
