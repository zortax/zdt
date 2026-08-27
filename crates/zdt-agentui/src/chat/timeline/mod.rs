//! The conversation's rows.
//!
//! A plain column, oldest at the top. The view follows new content only while the reader is at
//! the bottom: growth glides the view down, and a reader who scrolled up stays put until they
//! come back down. The scrollback is the mouse's alone; the keyboard lives in the sidebar and
//! the composer.
//!
//! # Cards
//!
//! A run of tool calls and thoughts is one card from its first step. A card only ever grows, so
//! the thread never gets shorter under the reader. The agent's own prose ends a run, and work
//! after the prose opens a new card below it. `[agent] activity = "verbose"` turns the cards off
//! and draws every call and every thought as a row of its own.

mod activity;
mod diff;
mod glyph;
mod think;
mod user;
mod work;

pub use crate::chat::timeline::glyph::tool_glyph_for;

use std::collections::HashSet;

use zdt_agent::thread::{ItemKind, LIVE_ASSISTANT};
use zdt_icons::{self as icons, IconProps};
use zdt_view::markdown::MarkdownProps;
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::{component, view};

use crate::chat::timeline::activity::ActivityCardProps;
use crate::chat::timeline::diff::DiffRowProps;
use crate::chat::timeline::think::ThinkRowProps;
use crate::chat::timeline::user::UserRowProps;
use crate::chat::timeline::work::WorkRowProps;
use crate::{AgentUi, use_agent};

/// One stretch of the timeline: a row of its own, or a run of work and thought.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Seg {
    /// One row.
    One(i64),
    /// A run of work, named by its first row.
    Card(i64),
}

impl Seg {
    /// A stable name for the keyed list.
    fn key(self) -> (u8, i64) {
        match self {
            Self::One(id) => (0, id),
            Self::Card(id) => (1, id),
        }
    }
}

/// Whether a row of `kind` belongs inside a card.
fn is_work(kind: ItemKind) -> bool {
    matches!(kind, ItemKind::Tool | ItemKind::Task | ItemKind::Thinking)
}

/// Every stretch of `rows`, oldest first.
///
/// Purely positional: a run of work is a card whatever has finished, so a step landing changes
/// only what a card says and never how the thread is cut up.
fn segment(rows: &[(i64, ItemKind)], grouped: bool) -> Vec<Seg> {
    if !grouped {
        return rows.iter().map(|(id, _)| Seg::One(*id)).collect();
    }
    let mut segments = Vec::with_capacity(rows.len());
    let mut inside = false;
    for (id, kind) in rows {
        if is_work(*kind) {
            if !inside {
                segments.push(Seg::Card(*id));
                inside = true;
            }
        } else {
            segments.push(Seg::One(*id));
            inside = false;
        }
    }
    segments
}

/// The run `anchor` begins: itself and every work row straight after it.
fn run_at(rows: &[(i64, ItemKind)], anchor: i64) -> Vec<i64> {
    rows.iter()
        .skip_while(|(id, _)| *id != anchor)
        .take_while(|(_, kind)| is_work(*kind))
        .map(|(id, _)| *id)
        .collect()
}

/// Every row of the watched thread, as an id beside its kind. Tracked.
fn kinds(agent: &AgentUi) -> Vec<(i64, ItemKind)> {
    agent
        .client()
        .order()
        .into_iter()
        .filter_map(|id| {
            let row = agent.client().row(id)?;
            Some((id, row.with(|item| item.kind)))
        })
        .collect()
}

/// Every row, newest at the bottom.
// The list macro takes a closure by construction, so the one it is handed here is not redundant.
#[allow(clippy::redundant_closure)]
#[component]
pub fn Timeline(
    /// The scroll container, for whoever gives the timeline the keyboard.
    node: NodeRef,
) -> impl IntoView {
    use zdt_view::Erase;

    let agent = use_agent();

    // Which cards are open, by anchor. Item ids are unique across threads, and the timeline is
    // never unmounted for a thread change, so what somebody opened stays open.
    let opened: RwSignal<HashSet<i64>, LocalStorage> = RwSignal::new_local(HashSet::new());

    let segments = {
        let agent = agent.clone();
        move || segment(&kinds(&agent), agent.host().groups_activity())
    };

    let empty = {
        let agent = agent.clone();
        move || {
            let nothing = agent.client().order().is_empty();
            (agent.selected().is_none() || nothing).then(|| "true".to_owned())
        }
    };
    // "Working" is drawn while a turn runs and nothing of the answer is on screen. With streaming
    // off the answer is withheld until it is done, so the indicator stays for the whole turn. It
    // sits under the last card, and is what says the turn is alive between two steps.
    let working = {
        let agent = agent.clone();
        move || {
            let busy = agent
                .selected_shell()
                .is_some_and(|shell| shell.state.is_busy());
            let streaming = agent.host().streams_text()
                && agent.client().order().last() == Some(&LIVE_ASSISTANT);
            (!busy || streaming).then(|| "none".to_owned())
        }
    };

    // Following the bottom. `pinned` says the reader is there; content growing while pinned
    // glides the view down, and only an upward scroll of the reader's own unpins it. The two
    // are told apart by what moved: growth changes the extent, a wheel changes the offset.
    //
    // A window resize moves the extent as well — the port itself, and the content as its text
    // re-wraps — and it arrives once per frame of a drag. Chasing that with the glide re-targets
    // an animation on every step and the bottom visibly falls behind the pointer, so a step whose
    // *port* moved pins the bottom where it stands and the glide is kept for what it was made
    // for: new content arriving in a port that is holding still. A width-only drag is still a
    // port move — the content re-wraps because the port changed shape — which is why both of the
    // port's axes are watched and the content's height alone means growth.
    let position = node.observe_scroll();
    let pinned = std::rc::Rc::new(std::cell::Cell::new(true));
    let following = {
        let pinned = std::rc::Rc::clone(&pinned);
        let extent = std::cell::Cell::new((0.0f32, 0.0f32, 0.0f32));
        let offset_seen = std::cell::Cell::new(0.0f32);
        zgui::reactive::RenderEffect::new(move |_| {
            let at = position.get();
            let content = at.content_size.height.0;
            let port = (at.scrollport.width.0, at.scrollport.height.0);
            let limit = (content - port.1).max(0.0);
            let offset = at.offset.y.0;

            let (content_seen, port_seen_w, port_seen_h) = extent.get();
            let resized = port != (port_seen_w, port_seen_h);
            let grew = resized || content != content_seen;
            extent.set((content, port.0, port.1));
            let up = offset < offset_seen.get() - 0.5;
            offset_seen.set(offset);

            if grew {
                if pinned.get() && offset < limit {
                    node.scroll_to(
                        ScrollTarget::Offset(zgui::geom::Point::new(
                            zgui::geom::DevicePx(0.0),
                            zgui::geom::DevicePx(limit),
                        )),
                        if resized {
                            ScrollBehavior::Instant
                        } else {
                            ScrollBehavior::Smooth
                        },
                    );
                }
            } else if up {
                pinned.set(false);
            } else if offset >= limit - 4.0 {
                pinned.set(true);
            }
        })
    };
    on_cleanup_local(move || drop(following));

    // A different thread starts at its bottom, pinned, whatever the last one showed.
    let landing = {
        let agent = agent.clone();
        let pinned = std::rc::Rc::clone(&pinned);
        zgui::reactive::RenderEffect::new(move |_| {
            let _ = agent.selected();
            pinned.set(true);
            let at = position.get_untracked();
            let limit = (at.content_size.height.0 - at.scrollport.height.0).max(0.0);
            node.scroll_to(
                ScrollTarget::Offset(zgui::geom::Point::new(
                    zgui::geom::DevicePx(0.0),
                    zgui::geom::DevicePx(limit),
                )),
                ScrollBehavior::Instant,
            );
        })
    };
    on_cleanup_local(move || drop(landing));

    let on_key = {
        let agent = agent.clone();
        move |event: &mut EventCx<'_, events::KeyDown>| {
            if agent.host().key(event, event.modifiers, crate::REGION_CHAT) {
                event.prevent_default();
                event.stop_propagation();
            }
        }
    };
    let take_focus = {
        let agent = agent.clone();
        move |_: &mut EventCx<'_, events::FocusIn>| {
            agent.chat_focused();
        }
    };

    view! {
        scroll(
            class = "agent-log",
            node_ref = node,
            tabindex = Focus::Programmatic,
            a11y:role = Role::Log,
            a11y:label = "Conversation",
            on:key_down = on_key,
            on:focus_in = take_focus
        ) {
            for seg in move || segments(), key = |seg: &Seg| seg.key() {
                {
                    match seg {
                        Seg::One(id) => view! { LogRow(id = id) }.any(),
                        Seg::Card(anchor) => view! {
                            box(class = "agent-log__row agent-log__row--card") {
                                ActivityCard(anchor = anchor, opened = opened)
                            }
                        }
                        .any(),
                    }
                }
            }
            row(class = "agent-log__working", style:display = working) {
                // Hidden by `display`, so the style animation stops with the row.
                Icon(icon = icons::LOADER_CIRCLE, class = "icon--sm zdt-spin")
                label(class = "muted") {"Working\u{2026}"}
            }
            box(class = "agent-log__empty", attr:data-on = empty) {
                label(class = "muted") {"No conversation yet. Type below to start one."}
            }
        }
    }
}

/// One row.
///
/// Prose that is finished is parsed once and never again; prose still streaming is a plain run
/// of text whose binding follows the row's own signal, so a delta re-parses nothing. Tool rows
/// follow their signal whole: they move a handful of times, not by the word.
#[component]
fn LogRow(
    /// Which row, within the watched thread.
    id: i64,
) -> impl IntoView {
    use zdt_view::Erase;

    let agent = use_agent();
    let row = agent.client().row(id);

    let kind = row.map_or(ItemKind::Unknown, |row| {
        row.with_untracked(|item| item.kind)
    });
    let class = match kind {
        ItemKind::User => "agent-log__row agent-log__row--user",
        ItemKind::Assistant => "agent-log__row agent-log__row--assistant",
        ItemKind::Thinking => "agent-log__row agent-log__row--thinking",
        ItemKind::Tool | ItemKind::Task => "agent-log__row agent-log__row--work",
        ItemKind::Diff => "agent-log__row agent-log__row--diff",
        ItemKind::Unknown => "agent-log__row",
    };

    let mut hidden = false;
    let body = match (kind, row) {
        (ItemKind::Tool | ItemKind::Task, Some(row)) => view! { WorkRow(row = row) }.any(),
        (ItemKind::Thinking, Some(row)) => view! { ThinkRow(row = row) }.any(),
        (ItemKind::Diff, Some(row)) => view! { DiffRow(row = row) }.any(),
        // Finished assistant prose is markdown, parsed at mount. Its text never changes: a
        // finished row is replaced, never edited.
        (ItemKind::Assistant, Some(row)) if row.with_untracked(|item| item.done) => {
            let blocks = row.with_untracked(|item| zdt_view::markdown::parse(&item.text));
            view! { Markdown(blocks = blocks) }.any()
        }
        // Streaming assistant prose follows its signal as plain text — when streaming is wanted.
        // Withheld, the row waits unseen and the finished markdown arrives whole in its place.
        (ItemKind::Assistant, Some(row)) => {
            if agent.host().streams_text() {
                let text = move || row.with(|item| item.text.clone());
                view! { label(class = "agent-log__text") {{text}} }.any()
            } else {
                hidden = true;
                view! { box {} }.any()
            }
        }
        // A person's own words, in an element a pointer can select in.
        (ItemKind::User, Some(row)) => view! { UserRow(row = row) }.any(),
        // Everything else follows the row's own signal as plain text.
        (_, Some(row)) => {
            let text = move || row.with(|item| item.text.clone());
            view! { label(class = "agent-log__text") {{text}} }.any()
        }
        (_, None) => view! { box {} }.any(),
    };
    let shown = move || hidden.then(|| "none".to_owned());

    view! {
        box(class = class, style:display = shown) {{body}}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kinds a thread's rows would report, in order.
    fn rows(kinds: &[(i64, ItemKind)]) -> Vec<(i64, ItemKind)> {
        kinds.to_vec()
    }

    #[test]
    fn one_tool_call_is_already_a_card() {
        // The point of the whole arrangement: nothing appears as a row and becomes a card later.
        let thread = rows(&[(1, ItemKind::User), (2, ItemKind::Tool)]);
        assert_eq!(segment(&thread, true), vec![Seg::One(1), Seg::Card(2)]);
    }

    #[test]
    fn a_run_of_work_and_thought_is_one_card() {
        let thread = rows(&[
            (1, ItemKind::User),
            (2, ItemKind::Thinking),
            (3, ItemKind::Tool),
            (4, ItemKind::Tool),
            (5, ItemKind::Task),
        ]);
        assert_eq!(segment(&thread, true), vec![Seg::One(1), Seg::Card(2)]);
    }

    #[test]
    fn the_agents_own_words_close_a_card() {
        let thread = rows(&[
            (1, ItemKind::Tool),
            (2, ItemKind::Tool),
            (3, ItemKind::Assistant),
            (4, ItemKind::Tool),
        ]);
        assert_eq!(
            segment(&thread, true),
            vec![Seg::Card(1), Seg::One(3), Seg::Card(4)]
        );
    }

    #[test]
    fn a_persons_words_and_a_turns_diff_close_a_card_too() {
        let thread = rows(&[
            (1, ItemKind::Tool),
            (2, ItemKind::Diff),
            (3, ItemKind::Tool),
            (4, ItemKind::User),
            (5, ItemKind::Tool),
        ]);
        assert_eq!(
            segment(&thread, true),
            vec![
                Seg::Card(1),
                Seg::One(2),
                Seg::Card(3),
                Seg::One(4),
                Seg::Card(5),
            ]
        );
    }

    #[test]
    fn a_card_keeps_its_name_as_its_run_grows() {
        // The keyed list holds the card mounted through this, which is what stops the thread
        // from jumping when a step lands.
        let mut thread = rows(&[(1, ItemKind::User), (2, ItemKind::Tool)]);
        let named = segment(&thread, true);
        for id in 3..=6 {
            thread.push((id, ItemKind::Tool));
            assert_eq!(segment(&thread, true), named);
            assert_eq!(run_at(&thread, 2), (2..=id).collect::<Vec<_>>());
        }
    }

    #[test]
    fn a_run_stops_at_the_first_row_that_is_not_work() {
        let thread = rows(&[
            (1, ItemKind::Tool),
            (2, ItemKind::Thinking),
            (3, ItemKind::Assistant),
            (4, ItemKind::Tool),
        ]);
        assert_eq!(run_at(&thread, 1), vec![1, 2]);
        assert_eq!(run_at(&thread, 4), vec![4]);
    }

    #[test]
    fn a_run_nobody_anchors_is_no_run_at_all() {
        let thread = rows(&[(1, ItemKind::Tool)]);
        assert!(run_at(&thread, 9).is_empty());
    }

    #[test]
    fn verbose_gives_every_row_back_its_own_line() {
        let thread = rows(&[
            (1, ItemKind::User),
            (2, ItemKind::Thinking),
            (3, ItemKind::Tool),
            (4, ItemKind::Tool),
        ]);
        assert_eq!(
            segment(&thread, false),
            vec![Seg::One(1), Seg::One(2), Seg::One(3), Seg::One(4)]
        );
    }

    #[test]
    fn a_thread_with_nothing_in_it_has_no_stretches() {
        assert!(segment(&[], true).is_empty());
        assert!(segment(&[], false).is_empty());
    }
}
