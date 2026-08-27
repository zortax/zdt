//! A run of work and thought, as one card.
//!
//! The card is a card from its first step. It only ever grows: a step finishing changes the
//! counter in its head and nothing else, so the thread never gets shorter under the reader. The
//! agent's own prose ends the run, and work after the prose opens a new card below it.

use std::collections::HashSet;

use zdt_agent::thread::{ItemKind, ItemStatus, ToolKind};
use zdt_icons::{self as icons, IconProps};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::{component, view};

use super::glyph::{span_text, tool_glyph};
use super::think::ThinkRowProps;
use super::work::WorkRowProps;
use crate::use_agent;

/// What one member of a card is, as far as the head is concerned.
///
/// Deliberately without the member's text: the facts feed a memo that holds the head still, and
/// a streamed word must not move it.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub(super) struct Facts {
    /// Whether it is a tool, a subagent, or a thought.
    pub kind: ItemKind,
    /// What sort of tool it is.
    pub tool: ToolKind,
    /// Where it stands.
    pub status: ItemStatus,
    /// Whether it is finished.
    pub done: bool,
    /// How long a thought took.
    pub elapsed_ms: u64,
}

impl Facts {
    /// Whether it is still going.
    fn running(&self) -> bool {
        match self.kind {
            ItemKind::Thinking => !self.done,
            _ => self.status == ItemStatus::Running,
        }
    }
}

/// One run of work and thought, slim until it is asked to open.
// The list macro takes a closure by construction, so the one it is handed here is not redundant.
#[allow(clippy::redundant_closure)]
#[component]
pub(super) fn ActivityCard(
    /// The run's first row, which names the card.
    anchor: i64,
    /// Which cards are open, by anchor.
    opened: RwSignal<HashSet<i64>, LocalStorage>,
    /// The thread's shape, shared by every card so a delta is judged once.
    shape: RwSignal<Vec<(i64, ItemKind)>, LocalStorage>,
) -> impl IntoView {
    let agent = use_agent();

    // The run re-derives itself from its anchor. The keyed list holds the card mounted while the
    // run grows, so a member list handed in at construction would never catch up.
    let ids = zdt_view::settled(move || shape.with(|rows| super::run_at(rows, anchor)));

    // The head's facts, settled. The rows stream, and every word notifies the whole row; the
    // facts leave the words out, so the value moves only when a step lands, finishes or fails —
    // and the head's closures below run then, and only then.
    let facts: RwSignal<Vec<Facts>, LocalStorage> = {
        let agent = agent.clone();
        zdt_view::settled(move || {
            ids.with(|ids| {
                ids.iter()
                    .filter_map(|id| agent.client().row(*id))
                    .map(|row| {
                        row.with(|item| Facts {
                            kind: item.kind,
                            tool: item.tool,
                            status: item.status,
                            done: item.done,
                            elapsed_ms: item.elapsed_ms,
                        })
                    })
                    .collect::<Vec<_>>()
            })
        })
    };

    let counter = move || facts.with(|facts| summarize(facts));
    let glyph = move || facts.with(|facts| card_glyph(facts));
    let state = move || Some(facts.with(|facts| card_state(facts)).to_owned());

    // What is happening right now, beside the counter. A thought says so in the counter already,
    // so only a running tool puts its name here. Settled on its own, because it does read the
    // running tool's words, and those move more often than they change what one line shows.
    let now: RwSignal<Option<(String, String)>, LocalStorage> = {
        let agent = agent.clone();
        zdt_view::settled(move || {
            ids.with(|ids| {
                ids.iter()
                    .filter_map(|id| agent.client().row(*id))
                    .find_map(|row| {
                        row.with(|item| {
                            let running = item.kind != ItemKind::Thinking
                                && item.status == ItemStatus::Running;
                            running.then(|| (item.name.clone(), item.text.clone()))
                        })
                    })
            })
        })
    };
    let now_name = move || now.with(|now| now.clone().map(|(name, _)| name).unwrap_or_default());
    let now_text = move || now.with(|now| now.clone().map(|(_, text)| text).unwrap_or_default());
    let now_shown = move || now.with(Option::is_none).then(|| "none".to_owned());

    let is_open = move || opened.with(|held| held.contains(&anchor));
    let chevron = move || {
        if is_open() {
            icons::CHEVRON_DOWN
        } else {
            icons::CHEVRON_RIGHT
        }
    };
    let steps_shown = move || (!is_open()).then(|| "none".to_owned());
    let toggle = move |event: &mut EventCx<'_, events::PointerDown>| {
        event.stop_propagation();
        opened.update(|held| {
            if !held.insert(anchor) {
                held.remove(&anchor);
            }
        });
    };

    // Only what is open is built. A closed card is one line whatever it holds.
    let steps = move || if is_open() { ids.get() } else { Vec::new() };
    let step = {
        let agent = agent.clone();
        move |id: i64| {
            use zdt_view::Erase;
            let Some(row) = agent.client().row(id) else {
                return view! { box {} }.any();
            };
            if row.with_untracked(|item| item.kind) == ItemKind::Thinking {
                view! { ThinkRow(row = row) }.any()
            } else {
                view! { WorkRow(row = row) }.any()
            }
        }
    };

    view! {
        column(
            class = "agent-card",
            attr:data-state = state,
            a11y:role = Role::Group,
            a11y:label = "Work"
        ) {
            row(class = "agent-card__head", on:pointer_down = toggle) {
                Icon(icon = Signal::derive_local(glyph), class = "icon--xs agent-card__glyph")
                label(class = "agent-card__count nowrap") {{counter}}
                row(class = "agent-card__now", style:display = now_shown) {
                    label(class = "agent-card__dot") {"\u{00b7}"}
                    label(class = "agent-card__name nowrap") {{now_name}}
                    label(class = "agent-card__summary nowrap") {{now_text}}
                }
                box(class = "fill") {}
                Icon(icon = Signal::derive_local(chevron), class = "icon--xs agent-card__chevron")
            }
            column(class = "agent-card__steps", style:display = steps_shown) {
                for id in move || steps(), key = |id: &i64| *id {
                    {step(id)}
                }
            }
        }
    }
}

/// How many of each sort of work a run holds.
#[derive(Default)]
struct Counts {
    execute: usize,
    edit: usize,
    read: usize,
    search: usize,
    web: usize,
    task: usize,
    plan: usize,
    other: usize,
}

impl Counts {
    /// The counts of `facts`.
    fn of(facts: &[Facts]) -> Self {
        let mut counts = Self::default();
        for one in facts {
            if one.kind == ItemKind::Task {
                counts.task += 1;
                continue;
            }
            if one.kind != ItemKind::Tool {
                continue;
            }
            match one.tool {
                ToolKind::Execute => counts.execute += 1,
                ToolKind::Edit => counts.edit += 1,
                ToolKind::Read => counts.read += 1,
                ToolKind::Search => counts.search += 1,
                ToolKind::Web => counts.web += 1,
                ToolKind::Plan => counts.plan += 1,
                ToolKind::Mcp | ToolKind::Other => counts.other += 1,
            }
        }
        counts
    }
}

/// `count` of `noun`, pluralised.
fn many(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

/// `text` with its first character in upper case.
fn upper_first(text: &str) -> String {
    let mut letters = text.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + letters.as_str(),
        None => String::new(),
    }
}

/// What a run did, in one line.
///
/// The clauses come in a fixed order, so a step landing only ever appends to the line. A line
/// whose words reordered as the work went on would be a line nobody could keep their place in.
pub(super) fn summarize(facts: &[Facts]) -> String {
    let counts = Counts::of(facts);
    let mut clauses: Vec<String> = Vec::new();
    if counts.execute > 0 {
        clauses.push(format!("ran {}", many(counts.execute, "command")));
    }
    if counts.edit > 0 {
        clauses.push(format!("changed {}", many(counts.edit, "file")));
    }
    if counts.read > 0 {
        clauses.push(format!("read {}", many(counts.read, "file")));
    }
    if counts.search > 0 {
        clauses.push(format!("searched {}", many(counts.search, "time")));
    }
    if counts.web > 0 {
        clauses.push(format!("fetched {}", many(counts.web, "page")));
    }
    if counts.task > 0 {
        clauses.push(many(counts.task, "subagent"));
    }
    if counts.plan > 0 {
        clauses.push("updated the plan".to_owned());
    }
    if counts.other > 0 {
        clauses.push(format!("used {}", many(counts.other, "tool")));
    }

    // The thinking comes last, after whatever was done with it. A run of pure thought is the
    // clock on its own.
    let mut line = if clauses.is_empty() {
        thought_word(facts)
    } else {
        clauses.extend(thought_clause(facts));
        upper_first(&clauses.join(", "))
    };
    if facts.iter().any(|one| one.status == ItemStatus::Failed) {
        line.push_str(", one failed");
    }
    line
}

/// What a run's thinking says beside the work it was spent on, when it is worth saying.
///
/// The time is every thought in the run added up. A thought too short to put a number on says
/// nothing here: the work is what the line is for, and the thought keeps its own row inside.
fn thought_clause(facts: &[Facts]) -> Option<String> {
    let thoughts: Vec<&Facts> = facts
        .iter()
        .filter(|one| one.kind == ItemKind::Thinking)
        .collect();
    if thoughts.is_empty() {
        return None;
    }
    if thoughts.iter().any(|one| !one.done) {
        return Some("thinking\u{2026}".to_owned());
    }
    let spent: u64 = thoughts.iter().map(|one| one.elapsed_ms).sum();
    (spent >= 1000).then(|| format!("thought for {}", span_text(spent)))
}

/// What a run holding nothing but thought says.
fn thought_word(facts: &[Facts]) -> String {
    if facts.iter().any(|one| !one.done) {
        return "Thinking\u{2026}".to_owned();
    }
    let spent: u64 = facts
        .iter()
        .filter(|one| one.kind == ItemKind::Thinking)
        .map(|one| one.elapsed_ms)
        .sum();
    if spent < 1000 {
        "Thought".to_owned()
    } else {
        format!("Thought for {}", span_text(spent))
    }
}

/// The outline a card wears: what broke, what is running, or what it did most of.
pub(super) fn card_glyph(facts: &[Facts]) -> &'static str {
    if facts.iter().any(|one| one.status == ItemStatus::Failed) {
        return icons::CIRCLE_ALERT;
    }
    if let Some(one) = facts.iter().find(|one| one.running()) {
        return match one.kind {
            ItemKind::Thinking => icons::BRAIN,
            _ => tool_glyph(one.kind, one.tool),
        };
    }
    match dominant(facts) {
        Some((kind, tool)) => tool_glyph(kind, tool),
        None => icons::BRAIN,
    }
}

/// Where a card stands, for the style sheet.
///
/// The sheet breathes the glyph of a running card off this word, the way every other animation
/// here is switched: by a state attribute on an ancestor, never by a flag on the animated
/// element itself. A card between two steps is settled; what says the turn is still going is
/// the working row under the thread.
pub(super) fn card_state(facts: &[Facts]) -> &'static str {
    if facts.iter().any(|one| one.status == ItemStatus::Failed) {
        "failed"
    } else if facts.iter().any(Facts::running) {
        "running"
    } else {
        "done"
    }
}

/// The sort of work a run holds most of, when it holds any.
///
/// A tie goes to whichever comes first in the counter's own order, so the outline and the words
/// agree about what the run was mostly doing.
fn dominant(facts: &[Facts]) -> Option<(ItemKind, ToolKind)> {
    let counts = Counts::of(facts);
    let ranked = [
        (counts.execute, ItemKind::Tool, ToolKind::Execute),
        (counts.edit, ItemKind::Tool, ToolKind::Edit),
        (counts.read, ItemKind::Tool, ToolKind::Read),
        (counts.search, ItemKind::Tool, ToolKind::Search),
        (counts.web, ItemKind::Tool, ToolKind::Web),
        (counts.task, ItemKind::Task, ToolKind::Other),
        (counts.plan, ItemKind::Tool, ToolKind::Plan),
        (counts.other, ItemKind::Tool, ToolKind::Other),
    ];
    ranked
        .into_iter()
        .filter(|(count, _, _)| *count > 0)
        .max_by_key(|(count, _, _)| *count)
        .map(|(_, kind, tool)| (kind, tool))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One finished tool row of `tool`'s sort.
    fn tool(tool: ToolKind) -> Facts {
        Facts {
            kind: ItemKind::Tool,
            tool,
            status: ItemStatus::Ok,
            done: true,
            ..Facts::default()
        }
    }

    /// One finished thought that took `ms`.
    fn thought(ms: u64) -> Facts {
        Facts {
            kind: ItemKind::Thinking,
            done: true,
            elapsed_ms: ms,
            ..Facts::default()
        }
    }

    #[test]
    fn a_counter_names_every_sort_of_work_it_holds() {
        let run = [
            tool(ToolKind::Execute),
            tool(ToolKind::Execute),
            tool(ToolKind::Execute),
            tool(ToolKind::Edit),
            tool(ToolKind::Edit),
        ];
        assert_eq!(summarize(&run), "Ran 3 commands, changed 2 files");
    }

    #[test]
    fn one_of_a_thing_is_said_in_the_singular() {
        assert_eq!(summarize(&[tool(ToolKind::Execute)]), "Ran 1 command");
        assert_eq!(summarize(&[tool(ToolKind::Edit)]), "Changed 1 file");
        assert_eq!(summarize(&[tool(ToolKind::Web)]), "Fetched 1 page");
    }

    /// One clause as what it is about, apart from how many: "ran 3 commands" is "ran command".
    fn about(clause: &str) -> String {
        let mut words: Vec<String> = clause
            .split_whitespace()
            .filter(|word| word.parse::<usize>().is_err())
            .map(str::to_lowercase)
            .collect();
        if let Some(last) = words.last_mut() {
            let singular = last.strip_suffix('s').unwrap_or(last).to_owned();
            *last = singular;
        }
        words.join(" ")
    }

    /// How many one clause counts.
    fn how_many(clause: &str) -> usize {
        clause
            .split_whitespace()
            .find_map(|word| word.parse::<usize>().ok())
            .unwrap_or(1)
    }

    #[test]
    fn a_step_landing_never_moves_a_clause_that_is_already_there() {
        // The line a reader is keeping their place in must not reorder underneath them, whatever
        // order the work actually arrived in. A new sort of work may open a clause ahead of the
        // ones already said, but no clause ever moves past another, drops out, or counts lower.
        let mut run = vec![tool(ToolKind::Edit)];
        let mut seen = summarize(&run);
        for next in [
            ToolKind::Execute,
            ToolKind::Read,
            ToolKind::Execute,
            ToolKind::Search,
            ToolKind::Edit,
            ToolKind::Web,
        ] {
            run.push(tool(next));
            let line = summarize(&run);
            let now: Vec<&str> = line.split(", ").collect();
            let mut at = 0;
            for clause in seen.split(", ") {
                let found = now[at..]
                    .iter()
                    .position(|later| about(later) == about(clause))
                    .map(|step| at + step);
                let found = found.unwrap_or_else(|| panic!("{seen:?} lost a clause in {line:?}"));
                assert!(
                    how_many(now[found]) >= how_many(clause),
                    "{seen:?} counted down in {line:?}"
                );
                at = found + 1;
            }
            seen = line;
        }
        assert_eq!(
            seen,
            "Ran 2 commands, changed 2 files, read 1 file, searched 1 time, fetched 1 page"
        );
    }

    #[test]
    fn a_run_of_pure_thought_says_how_long_it_thought_for() {
        assert_eq!(summarize(&[thought(12_000)]), "Thought for 12s");
        assert_eq!(summarize(&[thought(400)]), "Thought");
        assert_eq!(
            summarize(&[thought(6000), thought(6000)]),
            "Thought for 12s"
        );
    }

    #[test]
    fn a_thought_still_running_says_so() {
        let live = Facts {
            kind: ItemKind::Thinking,
            done: false,
            ..Facts::default()
        };
        assert_eq!(summarize(&[live]), "Thinking\u{2026}");
    }

    #[test]
    fn a_thought_beside_tools_is_named_after_them() {
        let run = [thought(12_000), tool(ToolKind::Execute)];
        assert_eq!(summarize(&run), "Ran 1 command, thought for 12s");
    }

    #[test]
    fn the_thinking_a_card_names_is_all_of_it_added_up() {
        let run = [
            thought(30_000),
            tool(ToolKind::Execute),
            thought(12_000),
            tool(ToolKind::Execute),
            tool(ToolKind::Edit),
        ];
        assert_eq!(
            summarize(&run),
            "Ran 2 commands, changed 1 file, thought for 42s"
        );
    }

    #[test]
    fn a_thought_still_running_beside_tools_says_so() {
        let going = Facts {
            kind: ItemKind::Thinking,
            done: false,
            ..Facts::default()
        };
        let run = [tool(ToolKind::Execute), tool(ToolKind::Execute), going];
        assert_eq!(summarize(&run), "Ran 2 commands, thinking\u{2026}");
    }

    #[test]
    fn a_thought_too_short_to_time_says_nothing_beside_tools() {
        // The work is what the line is for, and the thought keeps its own row inside the card.
        let run = [tool(ToolKind::Execute), thought(400)];
        assert_eq!(summarize(&run), "Ran 1 command");
    }

    #[test]
    fn the_thinking_is_always_the_last_thing_the_counter_says() {
        // So the clause that changes as the turn runs never moves the ones that do not.
        let run = [
            thought(5_000),
            tool(ToolKind::Read),
            tool(ToolKind::Execute),
        ];
        let line = summarize(&run);
        assert!(line.ends_with("thought for 5s"), "{line}");
        assert!(line.starts_with("Ran 1 command"), "{line}");
    }

    #[test]
    fn a_failure_is_named_at_the_end_of_the_counter() {
        let mut broken = tool(ToolKind::Execute);
        broken.status = ItemStatus::Failed;
        assert_eq!(
            summarize(&[tool(ToolKind::Execute), broken]),
            "Ran 2 commands, one failed"
        );
    }

    #[test]
    fn a_subagent_is_counted_whatever_tool_it_carries() {
        let mut runner = tool(ToolKind::Execute);
        runner.kind = ItemKind::Task;
        assert_eq!(summarize(&[runner]), "1 subagent");
    }

    #[test]
    fn a_running_card_breathes_and_a_settled_one_does_not() {
        let mut going = tool(ToolKind::Execute);
        going.status = ItemStatus::Running;
        going.done = false;
        assert_eq!(card_glyph(&[going.clone()]), icons::TERMINAL);
        assert_eq!(card_glyph(&[tool(ToolKind::Execute)]), icons::TERMINAL);
        assert_eq!(card_state(&[going]), "running");
        assert_eq!(card_state(&[tool(ToolKind::Execute)]), "done");
    }

    #[test]
    fn a_thinking_card_wears_a_brain() {
        let going = Facts {
            kind: ItemKind::Thinking,
            done: false,
            ..Facts::default()
        };
        assert_eq!(card_glyph(&[going]), icons::BRAIN);
        assert_eq!(card_glyph(&[thought(12_000)]), icons::BRAIN);
    }

    #[test]
    fn a_failure_is_what_the_card_wears_however_much_else_ran() {
        let mut broken = tool(ToolKind::Read);
        broken.status = ItemStatus::Failed;
        let run = [tool(ToolKind::Execute), broken];
        assert_eq!(card_glyph(&run), icons::CIRCLE_ALERT);
        assert_eq!(card_state(&run), "failed");
    }

    #[test]
    fn a_settled_card_wears_the_work_it_did_most_of() {
        let run = [
            tool(ToolKind::Read),
            tool(ToolKind::Read),
            tool(ToolKind::Execute),
        ];
        assert_eq!(card_glyph(&run), icons::EYE);
    }
}
