//! The plans: the one proposed, and the checklist underway.

use zdt_agent::todo::TodoState;
use zdt_icons::{self as icons, IconProps};
use zdt_view::markdown::MarkdownProps;
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::{component, view};

use crate::use_agent;

/// The proposed plan, waiting on a person.
///
/// An empty send carries it out; a typed send refines it. The card only reminds of both.
#[component]
pub fn PlanCard() -> impl IntoView {
    use zdt_view::Erase;

    let agent = use_agent();

    let implementing = {
        let agent = agent.clone();
        move |event: &mut EventCx<'_, events::PointerDown>| {
            event.stop_propagation();
            agent.implement();
        }
    };

    let card = {
        let agent = agent.clone();
        move || {
            let Some(markdown) = agent.client().plan() else {
                return ().any();
            };
            let blocks = zdt_view::markdown::parse(&markdown);
            let carry = implementing.clone();
            view! {
                    column(class = "agent-plan") {
                        row(class = "agent-plan__title") {
                            Icon(icon = icons::SPARKLES, class = "icon--sm")
                            label {"Proposed plan"}
                            box(class = "fill") {}
                            control(
                                class = "agent-plan__go",
                                tabindex = Focus::Programmatic,
                                a11y:label = "Carry the plan out",
                                on:pointer_down = carry
                            ) {
                                label {"Implement"}
                            }
                        }
                        scroll(class = "agent-plan__body") {
                            Markdown(blocks = blocks)
                        }
                        label(class = "agent-plan__hint muted") {
                            "empty \u{23ce} implements \u{00b7} typed \u{23ce} refines"
                        }
                    }
            }
            .any()
        }
    };

    view! {
        {card}
    }
}

/// The checklist the turn keeps, folded to one line until it is opened.
#[component]
pub fn TodoStrip() -> impl IntoView {
    use zdt_view::Erase;

    let agent = use_agent();
    let opened: RwSignal<bool, LocalStorage> = RwSignal::new_local(false);

    let toggle = move |event: &mut EventCx<'_, events::PointerDown>| {
        event.stop_propagation();
        opened.update(|held| *held = !*held);
    };

    let strip = {
        let agent = agent.clone();
        move || {
            let todos = agent.client().todos();
            if todos.is_empty() {
                return ().any();
            }
            let done = todos
                .iter()
                .filter(|todo| todo.state == TodoState::Done)
                .count();
            let whole = todos.len();
            let active = todos
                .iter()
                .find(|todo| todo.state == TodoState::Active)
                .map(|todo| todo.text.clone())
                .unwrap_or_default();
            let disclosed = opened.get();
            let chevron = if disclosed {
                icons::CHEVRON_DOWN
            } else {
                icons::CHEVRON_RIGHT
            };
            let rows: Vec<TodoFacts> = if disclosed {
                todos
                    .iter()
                    .enumerate()
                    .map(|(at, todo)| TodoFacts {
                        at,
                        text: todo.text.clone(),
                        state: todo.state,
                    })
                    .collect()
            } else {
                Vec::new()
            };
            view! {
                column(class = "agent-todos") {
                    row(class = "agent-todos__head", on:pointer_down = toggle) {
                        Icon(icon = chevron, class = "icon--xs")
                        Icon(icon = icons::LIST_TODO, class = "icon--xs")
                        label {{format!("Plan {done}/{whole}")}}
                        label(class = "muted nowrap") {{active}}
                    }
                    column(class = "agent-todos__rows") {
                        for row in move || rows.clone(), key = |row: &TodoFacts| row.at {
                            TodoRow(todo = row)
                        }
                    }
                }
            }
            .any()
        }
    };

    view! {
        {strip}
    }
}

/// One step, as the strip draws it.
#[derive(Clone, PartialEq)]
struct TodoFacts {
    at: usize,
    text: String,
    state: TodoState,
}

/// One step's row.
#[component]
fn TodoRow(
    /// The step.
    todo: TodoFacts,
) -> impl IntoView {
    let glyph = match todo.state {
        TodoState::Done => icons::CIRCLE_CHECK,
        TodoState::Active => icons::CIRCLE_DASHED,
        TodoState::Pending | TodoState::Unknown => icons::CIRCLE,
    };
    let word = match todo.state {
        TodoState::Done => "done",
        TodoState::Active => "active",
        TodoState::Pending | TodoState::Unknown => "pending",
    };
    let text = todo.text;

    view! {
        row(class = "agent-todos__row", attr:data-state = move || Some(word.to_owned())) {
            Icon(icon = glyph, class = "icon--xs agent-todos__glyph")
            label(class = "nowrap") {{text}}
        }
    }
}
