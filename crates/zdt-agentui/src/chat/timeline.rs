//! The conversation's rows.
//!
//! A plain column, oldest at the top. The view follows new content only while the reader is at
//! the bottom: growth glides the view down, and a reader who scrolled up stays put until they
//! come back down. The scrollback is the mouse's alone; the keyboard lives in the sidebar and
//! the composer.
//!
//! # Folds
//!
//! Finished tool runs recede. A run of them from an earlier turn folds into one row; the current
//! turn keeps its newest step visible and folds the ones behind it. A fold holding a failure
//! says so and opens on a press.

use std::collections::HashSet;

use zdt_agent::thread::{ItemKind, ItemStatus, LIVE_ASSISTANT, TimelineItem, ToolKind};
use zdt_icons::{self as icons, IconProps};
use zdt_view::markdown::MarkdownProps;
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::{component, view};

use crate::use_agent;

/// One stretch of the timeline: a row of its own, or a folded run of finished steps.
#[derive(Clone, PartialEq)]
enum Seg {
    /// One row.
    One(i64),
    /// A folded run, named by its first row.
    Fold {
        /// The rows inside, oldest first.
        ids: Vec<i64>,
        /// Whether one of them failed.
        failed: bool,
    },
}

impl Seg {
    /// A stable name for the keyed list.
    fn key(&self) -> (u8, i64) {
        match self {
            Self::One(id) => (0, *id),
            Self::Fold { ids, .. } => (1, ids.first().copied().unwrap_or_default()),
        }
    }
}

/// How many finished steps in a row fold when the turn is done.
const FOLD_PAST: usize = 2;

/// How many keep the current turn's tail visible before the rest folds.
const FOLD_LIVE: usize = 4;

/// What a fold decides about one row.
struct RowFacts {
    id: i64,
    foldable: bool,
    failed: bool,
    user: bool,
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

    let expanded: RwSignal<HashSet<i64>, LocalStorage> = RwSignal::new_local(HashSet::new());

    let segments = {
        let agent = agent.clone();
        move || {
            let order = agent.client().order();
            let facts: Vec<RowFacts> = order
                .iter()
                .filter_map(|id| {
                    let row = agent.client().row(*id)?;
                    Some(row.with(|item| RowFacts {
                        id: *id,
                        foldable: matches!(item.kind, ItemKind::Tool | ItemKind::Task) && item.done,
                        failed: item.status == ItemStatus::Failed,
                        user: item.kind == ItemKind::User,
                    }))
                })
                .collect();
            let last_user = facts.iter().rposition(|row| row.user);
            let opened = expanded.get();
            fold(&facts, last_user, &opened)
        }
    };

    let empty = {
        let agent = agent.clone();
        move || {
            let nothing = agent.client().order().is_empty();
            (agent.selected().is_none() || nothing).then(|| "true".to_owned())
        }
    };
    // "Working" is drawn while a turn runs and nothing of the answer is on screen. With streaming
    // off the answer is withheld until it is done, so the indicator stays for the whole turn.
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

    let toggle_fold = move |key: i64| {
        expanded.update(|held| {
            if !held.insert(key) {
                held.remove(&key);
            }
        });
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
                        Seg::Fold { ids, failed } => {
                            let key = ids.first().copied().unwrap_or_default();
                            let count = ids.len();
                            let open = move |event: &mut EventCx<'_, events::PointerDown>| {
                                event.stop_propagation();
                                toggle_fold(key);
                            };
                            let danger = failed.then(|| "true".to_owned());
                            view! {
                                row(
                                    class = "agent-log__row agent-log__fold",
                                    attr:data-failed = danger,
                                    on:pointer_down = open
                                ) {
                                    Icon(icon = icons::CHEVRON_RIGHT, class = "icon--xs")
                                    label(class = "muted") {
                                        {move || if failed {
                                            format!("{count} steps, one failed")
                                        } else {
                                            format!("{count} steps")
                                        }}
                                    }
                                }
                            }
                            .any()
                        }
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

/// Folds finished runs, oldest first.
fn fold(facts: &[RowFacts], last_user: Option<usize>, opened: &HashSet<i64>) -> Vec<Seg> {
    let mut segments = Vec::new();
    let mut run: Vec<usize> = Vec::new();
    let mut at = 0;
    while at <= facts.len() {
        let foldable = facts.get(at).is_some_and(|row| row.foldable);
        if foldable {
            run.push(at);
            at += 1;
            continue;
        }
        if !run.is_empty() {
            let past = last_user.is_some_and(|user| *run.last().expect("not empty") < user);
            let keep_tail = !past;
            let threshold = if past { FOLD_PAST } else { FOLD_LIVE };
            let key = facts[run[0]].id;
            if run.len() >= threshold && !opened.contains(&key) {
                let folded: Vec<usize> = if keep_tail {
                    run[..run.len() - 1].to_vec()
                } else {
                    run.clone()
                };
                if folded.len() >= 2 {
                    segments.push(Seg::Fold {
                        ids: folded.iter().map(|at| facts[*at].id).collect(),
                        failed: folded.iter().any(|at| facts[*at].failed),
                    });
                    if keep_tail {
                        segments.push(Seg::One(facts[*run.last().expect("not empty")].id));
                    }
                } else {
                    segments.extend(run.iter().map(|at| Seg::One(facts[*at].id)));
                }
            } else {
                segments.extend(run.iter().map(|at| Seg::One(facts[*at].id)));
            }
            run.clear();
        }
        if let Some(row) = facts.get(at) {
            segments.push(Seg::One(row.id));
        }
        at += 1;
    }
    segments
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

/// One user message: the words in a real editable element, so a pointer can select in it.
///
/// Read-only in practice rather than in state, because the framework only selects in editable
/// elements. Motion and copy keys pass through, `y` copies the selection like a yank, and every
/// key that would change the text is eaten before the editing default runs.
#[component]
fn UserRow(
    /// The row's own signal.
    row: RwSignal<TimelineItem, LocalStorage>,
) -> impl IntoView {
    let agent = use_agent();
    let field = NodeRef::new();
    let clipboard = try_use_clipboard();

    // The words are the element's children: a finished user row is replaced, never edited.
    let text = row.with_untracked(|item| item.text.clone());

    let on_key = move |cx: &mut EventCx<'_, events::KeyDown>| {
        use zgui::vocab::{Key, NamedKey};
        // The bubbled key must never reach the chat's region bindings.
        cx.stop_propagation();
        let motion = matches!(
            cx.key,
            Key::Named(
                NamedKey::ArrowLeft
                    | NamedKey::ArrowRight
                    | NamedKey::ArrowUp
                    | NamedKey::ArrowDown
                    | NamedKey::Home
                    | NamedKey::End
                    | NamedKey::PageUp
                    | NamedKey::PageDown
                    | NamedKey::Shift
                    | NamedKey::Control
                    | NamedKey::Copy
            )
        );
        let copies = matches!(
            &cx.key,
            Key::Character(typed)
                if cx.modifiers.control() && matches!(typed.as_str(), "c" | "a")
        );
        if motion || copies {
            return;
        }
        let yanks = matches!(
            &cx.key,
            Key::Character(typed)
                if typed.as_str() == "y" && !cx.modifiers.control() && !cx.modifiers.alt()
        );
        if yanks {
            if let Some(range) = field.selection()
                && !range.is_empty()
                && let Some(clipboard) = clipboard.clone()
            {
                let text = row.with_untracked(|item| item.text.clone());
                if let Some(piece) = text.get(range) {
                    clipboard.set_text(ClipboardKind::Standard, piece.to_owned());
                }
            }
            cx.prevent_default();
            return;
        }
        // Everything else would edit; the words stay as they were said.
        cx.prevent_default();
    };
    // The chat scroll must not pull the focus back while a selection is being made.
    let taken = {
        let agent = agent.clone();
        move |cx: &mut EventCx<'_, events::FocusIn>| {
            cx.stop_propagation();
            agent.host().took_keyboard();
        }
    };

    view! {
        editor(
            class = "agent-log__text",
            node_ref = field,
            tabindex = Focus::Programmatic,
            a11y:role = Role::TextInput,
            a11y:label = "Your message",
            on:key_down = on_key,
            on:focus_in = taken
        ) {
            {text}
        }
    }
}

/// One tool or task, slim until it is asked to open.
#[component]
fn WorkRow(
    /// The row's own signal.
    row: RwSignal<TimelineItem, LocalStorage>,
) -> impl IntoView {
    let opened: RwSignal<bool, LocalStorage> = RwSignal::new_local(false);

    let status = move || row.with(|item| item.status);
    let status_word = move || Some(status().word().to_owned());
    let glyph = move || {
        row.with(|item| match item.status {
            ItemStatus::Running => icons::CIRCLE_DASHED,
            ItemStatus::Failed => icons::CIRCLE_ALERT,
            ItemStatus::Declined => icons::CIRCLE_X,
            ItemStatus::Ok | ItemStatus::Unknown => tool_glyph(item.kind, item.tool),
        })
    };
    let name = move || row.with(|item| item.name.clone());
    let summary = move || row.with(|item| item.text.clone());
    let has_detail = move || row.with(|item| !item.detail.is_empty());
    let detail = move || {
        if opened.get() && has_detail() {
            row.with(|item| item.detail.clone())
        } else {
            String::new()
        }
    };
    let detail_shown = move || (!opened.get() || !has_detail()).then(|| "none".to_owned());

    let toggle = move |event: &mut EventCx<'_, events::PointerDown>| {
        event.stop_propagation();
        opened.update(|held| *held = !*held);
    };

    view! {
        column(class = "agent-work", attr:data-status = status_word, on:pointer_down = toggle) {
            row(class = "agent-work__head") {
                Icon(icon = Signal::derive_local(glyph), class = "icon--xs agent-work__glyph")
                label(class = "agent-work__name nowrap") {{name}}
                label(class = "agent-work__summary muted nowrap") {{summary}}
            }
            label(class = "agent-work__detail", style:display = detail_shown) {{detail}}
        }
    }
}

/// One thinking segment: a quiet single line, the whole thought a press away.
///
/// The thought itself is never streamed into the timeline. While the segment runs the line shows
/// a spinner and a climbing clock; done, it says how long it took. A press opens the full text.
#[component]
fn ThinkRow(
    /// The row's own signal.
    row: RwSignal<TimelineItem, LocalStorage>,
) -> impl IntoView {
    let opened: RwSignal<bool, LocalStorage> = RwSignal::new_local(false);

    let running = move || row.with(|item| !item.done);

    // The clock. The daemon says how long the segment had already run when this view arrived,
    // and a local timer carries it forward, armed only while the segment runs.
    let carried = row.with_untracked(|item| item.elapsed_ms);
    let began = std::time::Instant::now();
    let shown_ms: RwSignal<u64, LocalStorage> = RwSignal::new_local(carried);
    let slot: std::rc::Rc<std::cell::RefCell<Option<zgui::view::time::IntervalHandle>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let ticking = {
        let slot = std::rc::Rc::clone(&slot);
        zgui::reactive::RenderEffect::new(move |_| {
            let on = running();
            *slot.borrow_mut() = (on && zgui::view::time::Timers::current().is_some()).then(|| {
                zgui::view::time::set_interval(std::time::Duration::from_millis(250), move || {
                    shown_ms.set(carried + began.elapsed().as_millis() as u64);
                })
            });
        })
    };
    on_cleanup_local(move || drop((ticking, slot)));

    let glyph = move || {
        if running() {
            icons::LOADER_CIRCLE
        } else {
            icons::LIGHTBULB
        }
    };
    let live = move || running().then(|| "true".to_owned());
    let word = move || {
        row.with(|item| {
            if !item.done {
                "Thinking\u{2026}".to_owned()
            } else if item.elapsed_ms < 1000 {
                "Thought".to_owned()
            } else {
                format!("Thought for {}", span_text(item.elapsed_ms))
            }
        })
    };
    let clock = move || {
        if running() {
            span_text(shown_ms.get())
        } else {
            String::new()
        }
    };

    let full = move || row.with(|item| item.text.clone());
    // A model that keeps its reasoning back leaves nothing to open.
    let text_shown =
        move || (!opened.get() || row.with(|item| item.text.is_empty())).then(|| "none".to_owned());
    let toggle = move |event: &mut EventCx<'_, events::PointerDown>| {
        event.stop_propagation();
        opened.update(|held| *held = !*held);
    };

    view! {
        column(class = "agent-think", attr:data-running = live, on:pointer_down = toggle) {
            row(class = "agent-think__head") {
                Icon(
                    icon = Signal::derive_local(glyph),
                    class = "icon--xs agent-think__glyph"
                )
                label(class = "agent-think__word") {{word}}
                box(class = "fill") {}
                label(class = "agent-think__clock") {{clock}}
            }
            label(class = "agent-think__text", style:display = text_shown) {{full}}
        }
    }
}

/// What one turn changed: a one-line card, the files a press away.
///
/// The head says how much moved. Open, it lists every file with its counts; a press on a file
/// opens it in the editor. Two quiet controls review the whole span line by line or put the turn
/// back.
// The list macro takes a closure by construction, so the one it is handed here is not redundant.
#[allow(clippy::redundant_closure)]
#[component]
fn DiffRow(
    /// The row's own signal.
    row: RwSignal<TimelineItem, LocalStorage>,
) -> impl IntoView {
    use zdt_agent::change::{FileStat, TurnDiff};

    let agent = use_agent();
    let opened: RwSignal<bool, LocalStorage> = RwSignal::new_local(false);

    let diff = move || row.with(|item| TurnDiff::decode(&item.detail).unwrap_or_default());
    let files = move || diff().files;
    // The head's pieces, split so the counts can wear the diff's colours.
    let word = move || {
        let count = files().len();
        format!("{count} file{}", if count == 1 { "" } else { "s" })
    };
    let added = move || {
        let total: u32 = files().iter().map(|file| file.added).sum();
        format!("+{total}")
    };
    let removed = move || {
        let total: u32 = files().iter().map(|file| file.removed).sum();
        format!("\u{2212}{total}")
    };
    let files_shown = move || (!opened.get()).then(|| "none".to_owned());

    let toggle = move |event: &mut EventCx<'_, events::PointerDown>| {
        event.stop_propagation();
        opened.update(|held| *held = !*held);
    };
    let review = {
        let agent = agent.clone();
        move |event: &mut EventCx<'_, events::PointerDown>| {
            event.stop_propagation();
            agent.review_turn(&diff());
        }
    };
    let revert = {
        let agent = agent.clone();
        move |event: &mut EventCx<'_, events::PointerDown>| {
            event.stop_propagation();
            agent.revert_turn(diff().turn);
        }
    };

    let open_file = {
        let agent = agent.clone();
        move |path: &str| {
            if let Some(shell) = agent.selected_shell() {
                agent.host().open_file(&shell.root.join(path), None);
            }
        }
    };

    view! {
        column(class = "agent-diffcard") {
            row(class = "agent-diffcard__head", on:pointer_down = toggle) {
                Icon(icon = icons::FILE_DIFF, class = "icon--xs agent-diffcard__glyph")
                label(class = "agent-diffcard__word") {{word}}
                label(class = "agent-added nowrap") {{added}}
                label(class = "agent-removed nowrap") {{removed}}
                box(class = "fill") {}
                control(
                    class = "agent-diffcard__act",
                    tabindex = Focus::Programmatic,
                    a11y:label = "Review the changes",
                    on:pointer_down = review
                ) {
                    Icon(icon = icons::EYE, class = "icon--xs")
                    label {"review"}
                }
                control(
                    class = "agent-diffcard__act",
                    tabindex = Focus::Programmatic,
                    a11y:label = "Revert the turn",
                    on:pointer_down = revert
                ) {
                    Icon(icon = icons::HISTORY, class = "icon--xs")
                    label {"revert"}
                }
            }
            column(class = "agent-diffcard__files", style:display = files_shown) {
                for file in move || files(), key = |file: &FileStat| file.path.clone() {
                    {diff_file_row(&file, open_file.clone())}
                }
            }
        }
    }
}

/// One file of a diff card: the path, its counts, and a press that opens it.
fn diff_file_row<F: Fn(&str) + Clone + 'static>(
    file: &zdt_agent::change::FileStat,
    open_file: F,
) -> impl IntoView + use<F> {
    let path = file.path.clone();
    let (added, removed) = if file.binary {
        (String::new(), String::new())
    } else {
        (
            format!("+{}", file.added),
            format!("\u{2212}{}", file.removed),
        )
    };
    let binary_shown = (!file.binary).then(|| "none".to_owned());
    let open = {
        let path = path.clone();
        move |event: &mut EventCx<'_, events::PointerDown>| {
            event.stop_propagation();
            open_file(&path);
        }
    };
    view! {
        row(class = "agent-diffcard__file", on:pointer_down = open) {
            label(class = "agent-diffcard__path nowrap") {{path.clone()}}
            box(class = "fill") {}
            label(class = "muted nowrap", style:display = binary_shown) {"binary"}
            label(class = "agent-added nowrap") {{added}}
            label(class = "agent-removed nowrap") {{removed}}
        }
    }
}

/// A span of time in a few characters: "12s", "3m14s", "1h2m".
fn span_text(ms: u64) -> String {
    let seconds = ms / 1000;
    if seconds >= 3600 {
        format!("{}h{}m", seconds / 3600, (seconds % 3600) / 60)
    } else if seconds >= 60 {
        format!("{}m{}s", seconds / 60, seconds % 60)
    } else {
        format!("{seconds}s")
    }
}

/// The outline for a tool of `tool`'s sort, for whoever draws one outside the timeline.
#[must_use]
pub fn tool_glyph_for(tool: ToolKind) -> &'static str {
    tool_glyph(ItemKind::Tool, tool)
}

/// The outline a healthy tool row carries.
fn tool_glyph(kind: ItemKind, tool: ToolKind) -> &'static str {
    if kind == ItemKind::Task {
        return icons::BOT;
    }
    match tool {
        ToolKind::Read => icons::EYE,
        ToolKind::Edit => icons::PENCIL,
        ToolKind::Execute => icons::TERMINAL,
        ToolKind::Search => icons::SEARCH,
        ToolKind::Web => icons::GLOBE,
        ToolKind::Plan => icons::LIST_TODO,
        ToolKind::Mcp => icons::PLUG,
        ToolKind::Other => icons::WRENCH,
    }
}
