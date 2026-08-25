//! The thread sidebar.
//!
//! A flat list over every project, newest first, and the list never reorders on activity: a row
//! holds its place from open until it goes, so the screen only moves when somebody does
//! something. Status lives in the row, never in the order.
//!
//! # Motion
//!
//! Every movement here runs off a timer the sidebar arms only while something moves: the
//! working spinner turns, the waiting icon breathes, and the ages tick. A style animation would
//! ask the renderer for frames for ever; an interval behind a still list is the same waste.

use zdt_agent::thread::{ThreadShell, ThreadState};
use zdt_icons::{self as icons, IconProps};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::{component, view};
use zgui_ui::prelude::*;

use crate::state::SideRow;
use crate::use_agent;

/// How a thread stands, as the row says it.
///
/// An open ask outranks the work behind it, and a broken turn outranks a plan nobody took yet.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Standing {
    /// A turn runs.
    Working,
    /// The turn stopped to ask something.
    Waiting,
    /// A proposed plan waits to be taken.
    Plan,
    /// The last turn broke.
    Failed,
    /// Nothing running, nothing owed.
    Idle,
}

impl Standing {
    /// The one for `shell`.
    fn of(shell: &ThreadShell) -> Self {
        if shell.asking > 0 {
            Self::Waiting
        } else if shell.is_working() {
            Self::Working
        } else if shell.state == ThreadState::Failed {
            Self::Failed
        } else if shell.planned {
            Self::Plan
        } else {
            Self::Idle
        }
    }

    /// What the status line says.
    fn word(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Waiting => "waiting for input",
            Self::Plan => "plan ready",
            Self::Failed => "failed",
            Self::Idle => "idle",
        }
    }

    /// The word the style sheet colours by.
    fn tone(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Waiting => "waiting",
            Self::Plan => "plan",
            Self::Failed => "failed",
            Self::Idle => "idle",
        }
    }

    /// The status line's outline.
    fn glyph(self) -> &'static str {
        match self {
            Self::Working => icons::LOADER_CIRCLE,
            Self::Waiting => icons::CIRCLE_QUESTION,
            Self::Plan => icons::SPARKLES,
            Self::Failed => icons::CIRCLE_ALERT,
            Self::Idle => icons::CIRCLE,
        }
    }
}

/// The sidebar.
///
/// `node` is where the keyboard lands; the editor around the surface registers it as the agent
/// spot's sink.
// The list macro takes a closure by construction, so the one it is handed here is not redundant.
#[allow(clippy::redundant_closure)]
#[component]
pub fn AgentSidebar(
    /// Where the keyboard lands.
    node: NodeRef,
) -> impl IntoView {
    let agent = use_agent();
    let window = use_window();

    let open = {
        let agent = agent.clone();
        move || agent.is_open().then(|| "true".to_owned())
    };
    let focused = {
        let agent = agent.clone();
        move || agent.host().has_keyboard().then(|| "true".to_owned())
    };
    let showing_agent = {
        let agent = agent.clone();
        move || (agent.screen() == crate::Screen::Agent).then(|| "true".to_owned())
    };
    let rows = {
        let agent = agent.clone();
        move || agent.rows()
    };
    let connected = {
        let agent = agent.clone();
        move || (!agent.client().is_connected()).then(|| "true".to_owned())
    };
    let standing = {
        let agent = agent.clone();
        move || {
            if agent.client().is_connected() {
                return String::new();
            }
            agent
                .client()
                .standing()
                .unwrap_or_else(|| "connecting to the daemon\u{2026}".to_owned())
        }
    };

    // The shared beats every row draws from. Each interval is armed only while a row needs it.
    let now: RwSignal<u64, LocalStorage> = RwSignal::new_local(zdt_core::state::now_ms());

    // The age tick: half a minute, while the sidebar is open and has rows.
    let ticking_slot: std::rc::Rc<std::cell::RefCell<Option<zgui::view::time::IntervalHandle>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let ticking = {
        let agent = agent.clone();
        let slot = std::rc::Rc::clone(&ticking_slot);
        zgui::reactive::RenderEffect::new(move |_| {
            let on = agent.is_open() && !agent.client().threads().is_empty();
            *slot.borrow_mut() = (on && zgui::view::time::Timers::current().is_some()).then(|| {
                zgui::view::time::set_interval(std::time::Duration::from_secs(30), move || {
                    now.set(zdt_core::state::now_ms());
                })
            });
            if on {
                now.set(zdt_core::state::now_ms());
            }
        })
    };
    on_cleanup_local(move || drop((ticking, ticking_slot)));

    let on_key = {
        let agent = agent.clone();
        move |event: &mut EventCx<'_, events::KeyDown>| {
            if agent.host().key(event, event.modifiers, crate::REGION) {
                event.prevent_default();
                event.stop_propagation();
            }
        }
    };
    let take_focus = {
        let agent = agent.clone();
        move |_: &mut EventCx<'_, events::FocusIn>| agent.host().took_keyboard()
    };

    // The face toggle: two pills, one lit. Pressing the one already lit changes nothing.
    let showing_editor = {
        let agent = agent.clone();
        move || (agent.screen() == crate::Screen::Editor).then(|| "true".to_owned())
    };
    let to_editor = {
        let agent = agent.clone();
        move |event: &mut EventCx<'_, events::PointerDown>| {
            event.stop_propagation();
            if agent.screen() == crate::Screen::Agent {
                agent.toggle_screen();
            }
        }
    };
    let to_agent = {
        let agent = agent.clone();
        move |event: &mut EventCx<'_, events::PointerDown>| {
            event.stop_propagation();
            if agent.screen() == crate::Screen::Editor {
                agent.toggle_screen();
            }
        }
    };

    view! {
        column(
            class = "agent-side",
            node_ref = node,
            tabindex = Focus::Programmatic,
            attr:data-open = open,
            attr:data-focused = focused,
            a11y:role = Role::List,
            a11y:label = "Agent threads",
            on:key_down = on_key,
            on:focus_in = take_focus
        ) {
            row(class = "agent-side__head", on:pointer_down = window.move_drag_handler()) {
                label(class = "agent-side__title nowrap") {"Agents"}
                box(class = "fill") {}
                row(class = "agent-side__faces", a11y:role = Role::TabList) {
                    control(
                        class = "agent-side__face",
                        tabindex = Focus::Programmatic,
                        a11y:label = "Editor view",
                        attr:data-on = showing_editor,
                        on:pointer_down = to_editor
                    ) {
                        Icon(icon = icons::CODE_XML, class = "icon--xs")
                        label(class = "nowrap") {"Editor"}
                    }
                    control(
                        class = "agent-side__face",
                        tabindex = Focus::Programmatic,
                        a11y:label = "Agent view",
                        attr:data-on = showing_agent,
                        on:pointer_down = to_agent
                    ) {
                        Icon(icon = icons::BOT, class = "icon--xs")
                        label(class = "nowrap") {"Agent"}
                    }
                }
            }
            SideTools()
            scroll(class = "agent-side__rows") {
                for row in move || rows(), key = |row: &SideRow| row.key() {
                    SideRowView(row = row.clone(), now = now)
                }
            }
            box(class = "fill") {}
            row(class = "agent-side__foot", attr:data-off = connected) {
                label(class = "muted") {{standing}}
            }
        }
    }
}

/// The strip under the header: the search over the list, and the button that starts a thread.
// The list macro takes a closure by construction, so the one it is handed here is not redundant.
#[allow(clippy::redundant_closure)]
#[component]
fn SideTools() -> impl IntoView {
    let agent = use_agent();

    // The search, bound both ways so clearing it from anywhere empties the field too.
    let typed = {
        let agent = agent.clone();
        Binding::controlled(
            Signal::derive_local({
                let agent = agent.clone();
                move || agent.filter()
            }),
            move |typed: String| agent.set_filter(typed),
        )
    };
    let agent = use_agent();
    let field = NodeRef::new();
    agent.register_search(field);
    // Every key is the field's: stopped before the sidebar's own keymap can both run a list
    // command and cancel the insert.
    let search_key = {
        let agent = agent.clone();
        move |cx: &mut EventCx<'_, events::KeyDown>| {
            use zgui::vocab::{Key, NamedKey};
            if matches!(cx.key, Key::Named(NamedKey::Escape)) {
                agent.set_filter(String::new());
                field.set_value("");
                agent.to_list();
                agent.host().focus_agent();
                cx.prevent_default();
            }
            cx.stop_propagation();
        }
    };
    // The field's own focus is its own claim: stopped here so the sidebar's grab does not put
    // the keyboard straight back on the list.
    let search_taken = {
        let agent = agent.clone();
        move |cx: &mut EventCx<'_, events::FocusIn>| {
            cx.stop_propagation();
            agent.to_filter();
            agent.host().took_keyboard();
        }
    };

    // The new-thread button and its providers, over the portal band. Everything the portal's
    // children read rides in `Copy` signals.
    let seat = NodeRef::new();
    let adding: RwSignal<bool, LocalStorage> = RwSignal::new_local(false);
    let held: RwSignal<Option<crate::AgentUi>, LocalStorage> =
        RwSignal::new_local(Some(agent.clone()));
    // The list: one row per instance, then one import row per provider that keeps its own
    // session store on disk.
    let entries = {
        let named = agent.host().instances();
        let mut rows: Vec<NewEntry> = named
            .iter()
            .map(|(name, provider)| NewEntry {
                instance: name.clone(),
                provider: provider.clone(),
                import: false,
            })
            .collect();
        for kind in ["claude", "codex"] {
            if let Some((name, provider)) = named.iter().find(|(_, provider)| provider == kind) {
                rows.push(NewEntry {
                    instance: name.clone(),
                    provider: provider.clone(),
                    import: true,
                });
            }
        }
        rows
    };
    let instances: RwSignal<std::rc::Rc<Vec<NewEntry>>, LocalStorage> =
        RwSignal::new_local(std::rc::Rc::new(entries));

    let open_add = move |event: &mut EventCx<'_, events::PointerDown>| {
        event.stop_propagation();
        adding.update(|held| *held = !*held);
    };
    let compute_place = move || {
        if !adding.get() {
            return None;
        }
        let chip = seat.window_bounds()?;
        let root = seat.window_root()?;
        let window = root.bounds()?;
        let scale = f64::from(seat.scale());
        let width = f64::from(window.size.width.0) / scale;
        let x = (f64::from(chip.origin.x.0 - window.origin.x.0) / scale)
            .clamp(0.0, (width - 240.0).max(0.0));
        let top = f64::from(chip.origin.y.0 - window.origin.y.0) / scale
            + f64::from(chip.size.height.0) / scale
            + 6.0;
        Some((x, top))
    };
    let placed: RwSignal<Option<(f64, f64)>, LocalStorage> = RwSignal::new_local(None);
    let placing = zgui::reactive::RenderEffect::new(move |_| {
        let fresh = compute_place();
        if placed.get_untracked() != fresh {
            placed.set(fresh);
        }
    });
    on_cleanup_local(move || drop(placing));

    let pop_shown = move || placed.get().is_none().then(|| "none".to_owned());
    let left = move || placed.get().map(|(x, _)| format!("{x:.0}px"));
    let top_css = move || placed.get().map(|(_, top)| format!("{top:.0}px"));
    let dismiss = move |event: &mut EventCx<'_, events::PointerDown>| {
        event.stop_propagation();
        adding.set(false);
    };
    let indexes = move || {
        if !adding.get() {
            return Vec::new();
        }
        (0..instances.with(|held| held.len())).collect::<Vec<usize>>()
    };

    view! {
        row(class = "agent-side__tools") {
            row(class = "agent-side__searchbox") {
                Icon(icon = icons::SEARCH, class = "icon--xs agent-side__searchmark")
                Input(
                    class = "agent-side__searchfield",
                    node_ref = field,
                    value = typed,
                    placeholder = "Search threads",
                    label = "Search threads",
                    on:key_down = search_key,
                    on:focus_in = search_taken
                )
            }
            control(
                class = "agent-side__add",
                node_ref = seat,
                tabindex = Focus::Programmatic,
                a11y:label = "New thread",
                on:pointer_down = open_add
            ) {
                Icon(icon = icons::PLUS, class = "icon--xs")
            }
        }
        Portal {
            box(class = "agent-pop__backdrop", style:display = pop_shown, on:pointer_down = dismiss) {}
            column(
                class = "agent-pop agent-side__newpop",
                style:display = pop_shown,
                style:left = left,
                style:top = top_css
            ) {
                for index in move || indexes(), key = |index: &usize| *index {
                    NewThreadRow(index = index, held = held, instances = instances, adding = adding)
                }
            }
        }
    }
}

/// One row of the new-thread list: an instance to start on, or a provider to import from.
#[derive(Clone, PartialEq, Eq, Debug)]
struct NewEntry {
    /// The instance the choice goes to.
    instance: String,
    /// Its harness word.
    provider: String,
    /// Whether choosing it imports an existing conversation rather than starting fresh.
    import: bool,
}

/// One provider the new-thread button offers.
#[component]
fn NewThreadRow(
    /// Which place in the list this row holds.
    index: usize,
    /// The surface, parked for the portal.
    held: RwSignal<Option<crate::AgentUi>, LocalStorage>,
    /// The instances, name beside harness word.
    instances: RwSignal<std::rc::Rc<Vec<NewEntry>>, LocalStorage>,
    /// Whether the list is open, closed by a choice.
    adding: RwSignal<bool, LocalStorage>,
) -> impl IntoView {
    let row = move || instances.with(|held| held.get(index).cloned());
    // One line: the provider's full name, and the instance's own name beside it only when it
    // says something the provider's does not.
    let name = move || {
        row()
            .map(|entry| {
                let label = provider_label(&entry.provider);
                let label = if label.is_empty() {
                    entry.instance.clone()
                } else {
                    label.to_owned()
                };
                if entry.import {
                    format!("Import from {label}\u{2026}")
                } else {
                    label
                }
            })
            .unwrap_or_default()
    };
    let detail = move || {
        row()
            .filter(|entry| {
                !entry.import
                    && entry.instance != entry.provider
                    && !provider_label(&entry.provider).is_empty()
            })
            .map(|entry| entry.instance)
            .unwrap_or_default()
    };
    let mark = move || row().and_then(|entry| zdt_icons::brand(&entry.provider));
    let mark_icon = move || mark().unwrap_or(icons::DOT);
    let mark_shown = move || mark().is_none().then(|| "none".to_owned());
    let fallback_shown = move || mark().is_some().then(|| "none".to_owned());

    let pick = {
        move |event: &mut EventCx<'_, events::PointerDown>| {
            event.stop_propagation();
            adding.set(false);
            let Some(agent) = held.with_untracked(Clone::clone) else {
                return;
            };
            let Some(entry) = row() else {
                return;
            };
            if entry.import {
                agent.import_from(entry.instance, entry.provider);
                return;
            }
            let Some(root) = agent.host().project_root() else {
                agent
                    .host()
                    .say("no session is on screen to start a thread in");
                return;
            };
            agent.create_in(root, entry.instance);
        }
    };

    view! {
        row(class = "agent-pop__row", on:pointer_down = pick) {
            Icon(
                icon = Signal::derive_local(mark_icon),
                class = "icon--xs icon--brand agent-pop__brand",
                style:display = mark_shown
            )
            Icon(
                icon = icons::BOT,
                class = "icon--xs agent-pop__brand",
                style:display = fallback_shown
            )
            label(class = "agent-pop__label nowrap") {{name}}
            label(class = "agent-pop__desc muted nowrap") {{detail}}
        }
    }
}

/// What the harness word is called where a person reads it.
fn provider_label(provider: &str) -> &'static str {
    match provider {
        "claude" => "Claude Code",
        "codex" => "Codex",
        "mock" => "Mock",
        _ => "",
    }
}

/// One sidebar row: a shelf's header, or a thread on it.
///
/// The match is safe at mount: a key is either a shelf's for ever or a thread's for ever, so a
/// row never changes sort under its key.
#[component]
fn SideRowView(
    /// The row.
    row: SideRow,
    /// The moment the ages are measured against.
    now: RwSignal<u64, LocalStorage>,
) -> impl IntoView {
    use zdt_view::Erase;
    match row {
        SideRow::Header(shelf) => view! {
            row(class = "agent-side__shelf") {
                label(class = "nowrap") {{shelf.word()}}
            }
        }
        .any(),
        SideRow::Thread(shell) => view! {
            AgentRow(thread = shell.id, now = now)
        }
        .any(),
    }
}

/// One thread's row: the name, the project, how it stands, and how long ago it moved.
///
/// Everything it draws is read inside tracked closures over its id: the list is keyed by id, and
/// a row is kept while its thread is, however the shells around it change.
#[component]
fn AgentRow(
    /// Which thread it stands for.
    thread: zdt_agent::thread::ThreadId,
    /// The moment the ages are measured against.
    now: RwSignal<u64, LocalStorage>,
) -> impl IntoView {
    let agent = use_agent();

    let shell = {
        let agent = agent.clone();
        move || agent.client().thread(thread)
    };
    let index = {
        let agent = agent.clone();
        move || agent.visible().iter().position(|held| held.id == thread)
    };
    let selected = {
        let agent = agent.clone();
        move || (agent.selected() == Some(thread)).then(|| "true".to_owned())
    };
    // The caret shows only while the list answers motions; otherwise the selection alone is
    // highlighted.
    let on_caret = {
        let (agent, index) = (agent.clone(), index.clone());
        move || (agent.caret_shown() && index() == Some(agent.at())).then(|| "true".to_owned())
    };
    // The caret's row keeps itself in view, so keyboard walks scroll the list.
    let card = NodeRef::new();
    let showing = {
        let on_caret = on_caret.clone();
        zgui::reactive::RenderEffect::new(move |_| {
            if on_caret().is_some() {
                card.scroll_to(ScrollTarget::IntoView, ScrollBehavior::Instant);
            }
        })
    };
    on_cleanup_local(move || drop(showing));
    let standing = {
        let shell = shell.clone();
        move || shell().as_ref().map_or(Standing::Idle, Standing::of)
    };
    // Whether the row is put away: parked rows recede, and the shelf name says how.
    let parked = {
        let shell = shell.clone();
        move || {
            shell()
                .filter(|shell| {
                    shell.settled
                        || shell.archived
                        || shell.snoozed_until > zdt_core::state::now_ms()
                })
                .map(|_| "true".to_owned())
        }
    };
    let tone = {
        let standing = standing.clone();
        move || Some(standing().tone().to_owned())
    };
    // The status word. A sleeping row says when it wakes, and a woke one says it woke.
    let word = {
        let (shell, standing) = (shell.clone(), standing.clone());
        move || {
            let Some(held) = shell() else {
                return String::new();
            };
            let at = now.get();
            if held.snoozed_until > at {
                return format!("zzz {}", age_of(held.snoozed_until, at));
            }
            if held.snoozed_until != 0 && standing() == Standing::Idle {
                return "woke".to_owned();
            }
            standing().word().to_owned()
        }
    };
    let glyph = {
        let (shell, standing) = (shell.clone(), standing.clone());
        move || {
            let asleep = shell().is_some_and(|held| held.snoozed_until > now.get());
            if asleep {
                icons::MOON
            } else {
                standing().glyph()
            }
        }
    };
    let title = {
        let shell = shell.clone();
        move || shell().map(|shell| shell.title).unwrap_or_default()
    };
    // The provider's mark, in front of the name: which agent lives here, at a glance. A brand
    // mark is filled art and the outline fallback is stroked, so each is its own element.
    let mark = {
        let shell = shell.clone();
        move || shell().and_then(|shell| zdt_icons::brand(&shell.provider))
    };
    let mark_icon = {
        let mark = mark.clone();
        move || mark().unwrap_or(icons::DOT)
    };
    let mark_shown = {
        let mark = mark.clone();
        move || mark().is_none().then(|| "none".to_owned())
    };
    let fallback_shown = move || mark().is_some().then(|| "none".to_owned());
    let project = {
        let shell = shell.clone();
        move || shell().map(|shell| shell.project).unwrap_or_default()
    };
    let branch = {
        let shell = shell.clone();
        move || shell().map(|shell| shell.branch).unwrap_or_default()
    };
    let branch_shown = {
        let branch = branch.clone();
        move || branch().is_empty().then(|| "none".to_owned())
    };
    let in_worktree = {
        let shell = shell.clone();
        move || {
            shell()
                .is_none_or(|shell| !shell.worktree)
                .then(|| "none".to_owned())
        }
    };
    let changed = {
        let shell = shell.clone();
        move || {
            shell()
                .map(|shell| shell.changed)
                .filter(|changed| !changed.is_empty())
        }
    };
    let stat_added = {
        let changed = changed.clone();
        move || {
            changed()
                .map(|changed| format!("+{}", changed.added))
                .unwrap_or_default()
        }
    };
    let stat_removed = {
        let changed = changed.clone();
        move || {
            changed()
                .map(|changed| format!("\u{2212}{}", changed.removed))
                .unwrap_or_default()
        }
    };
    let stat_shown = {
        let changed = changed.clone();
        move || changed().is_none().then(|| "none".to_owned())
    };
    let age = {
        let shell = shell.clone();
        move || {
            shell()
                .map(|shell| age_of(now.get(), shell.updated_at_ms))
                .unwrap_or_default()
        }
    };

    let open = {
        let (agent, index) = (agent.clone(), index.clone());
        move |event: &mut EventCx<'_, events::PointerDown>| {
            event.stop_propagation();
            if let Some(index) = index() {
                agent.open_at(index);
            }
        }
    };

    // The row's own marks: pinned, unread news, an unsent draft.
    let pinned_shown = {
        let shell = shell.clone();
        move || {
            shell()
                .is_none_or(|shell| shell.pinned <= 0.0)
                .then(|| "none".to_owned())
        }
    };
    let unread_shown = {
        let shell = shell.clone();
        move || {
            shell()
                .is_none_or(|shell| !shell.unread)
                .then(|| "none".to_owned())
        }
    };
    let draft_shown = {
        let shell = shell.clone();
        move || {
            shell()
                .is_none_or(|shell| shell.draft.is_empty())
                .then(|| "none".to_owned())
        }
    };

    // The quick actions that swap in for the status on hover: a short snooze, then settle. Each
    // says what pressing it does to this row, so a parked row offers the reverse.
    //
    // Each button's hot state is its own signal, written by enter and leave. An attribute drives
    // the styling, so the label and the icon restyle together and a leave always clears it.
    let hot_settle: RwSignal<bool, LocalStorage> = RwSignal::new_local(false);
    let hot_snooze: RwSignal<bool, LocalStorage> = RwSignal::new_local(false);
    let settle_hot = move || hot_settle.get().then(|| "true".to_owned());
    let snooze_hot = move || hot_snooze.get().then(|| "true".to_owned());
    let settle_word = {
        let shell = shell.clone();
        move || {
            if shell().is_some_and(|held| held.settled) {
                "unsettle".to_owned()
            } else {
                "settle".to_owned()
            }
        }
    };
    let snooze_word = {
        let shell = shell.clone();
        move || {
            let asleep = shell().is_some_and(|held| held.snoozed_until > zdt_core::state::now_ms());
            if asleep {
                "wake".to_owned()
            } else {
                "snooze".to_owned()
            }
        }
    };
    let quick_settle = {
        let (agent, index) = (agent.clone(), index.clone());
        move |event: &mut EventCx<'_, events::PointerDown>| {
            event.stop_propagation();
            if let Some(index) = index() {
                agent.go_to(index);
                agent.settle_toggle();
            }
        }
    };
    let quick_snooze = {
        let (agent, index) = (agent.clone(), index.clone());
        let shell = shell.clone();
        move |event: &mut EventCx<'_, events::PointerDown>| {
            event.stop_propagation();
            let Some(index) = index() else {
                return;
            };
            agent.go_to(index);
            // Asleep already: the press wakes it. Awake: an hour of quiet.
            let asleep = shell().is_some_and(|held| held.snoozed_until > zdt_core::state::now_ms());
            if asleep {
                agent.snooze_until(0);
            } else {
                agent.snooze_until(zdt_core::state::now_ms() + 3_600_000);
            }
        }
    };

    view! {
        column(
            class = "agent-side__row",
            node_ref = card,
            attr:data-selected = selected,
            attr:data-caret = on_caret,
            attr:data-tone = tone,
            attr:data-parked = parked,
            a11y:role = Role::ListItem,
            on:pointer_down = open
        ) {
            row(class = "agent-side__top") {
                Icon(
                    icon = Signal::derive_local(mark_icon),
                    class = "icon--xs icon--brand agent-side__brand",
                    style:display = mark_shown
                )
                Icon(
                    icon = icons::BOT,
                    class = "icon--xs agent-side__brand",
                    style:display = fallback_shown
                )
                label(class = "agent-side__name nowrap") {{title}}
                Icon(
                    icon = icons::PIN,
                    class = "icon--xs agent-side__mark agent-side__mark--pin",
                    style:display = pinned_shown
                )
                Icon(
                    icon = icons::PENCIL,
                    class = "icon--xs agent-side__mark muted",
                    style:display = draft_shown
                )
                box(class = "agent-side__dot", style:display = unread_shown) {}
                box(class = "fill") {}
                row(class = "agent-side__state") {
                    Icon(
                        icon = Signal::derive_local(glyph),
                        class = "icon--xs agent-side__glyph"
                    )
                    label(class = "nowrap") {{word}}
                }
                row(class = "agent-side__quick") {
                    control(
                        class = "agent-side__act",
                        tabindex = Focus::Programmatic,
                        a11y:label = "Snooze",
                        attr:data-hot = snooze_hot,
                        on:pointer_enter = move |_: &mut EventCx<'_, events::PointerEnter>| {
                            hot_snooze.set(true);
                        },
                        on:pointer_leave = move |_: &mut EventCx<'_, events::PointerLeave>| {
                            hot_snooze.set(false);
                        },
                        on:pointer_down = quick_snooze
                    ) {
                        Icon(icon = icons::CLOCK, class = "icon--xs")
                        label(class = "nowrap") {{snooze_word}}
                    }
                    control(
                        class = "agent-side__act",
                        tabindex = Focus::Programmatic,
                        a11y:label = "Settle",
                        attr:data-hot = settle_hot,
                        on:pointer_enter = move |_: &mut EventCx<'_, events::PointerEnter>| {
                            hot_settle.set(true);
                        },
                        on:pointer_leave = move |_: &mut EventCx<'_, events::PointerLeave>| {
                            hot_settle.set(false);
                        },
                        on:pointer_down = quick_settle
                    ) {
                        Icon(icon = icons::CHECK, class = "icon--xs")
                        label(class = "nowrap") {{settle_word}}
                    }
                }
            }
            row(class = "agent-side__under") {
                label(class = "agent-side__project muted nowrap") {{project}}
                row(class = "agent-side__branch muted", style:display = branch_shown) {
                    Icon(icon = icons::GIT_BRANCH, class = "icon--xs")
                    label(class = "nowrap") {{branch}}
                }
                Icon(
                    icon = icons::FOLDER_GIT,
                    class = "icon--xs agent-side__wt muted",
                    style:display = in_worktree
                )
                row(class = "agent-side__stat nowrap", style:display = stat_shown) {
                    label(class = "agent-added") {{stat_added}}
                    label(class = "agent-removed") {{stat_removed}}
                }
                box(class = "fill") {}
                label(class = "agent-side__age muted nowrap") {{age}}
            }
        }
    }
}

/// How long ago `then` was, in one short word.
fn age_of(now: u64, then: u64) -> String {
    let seconds = now.saturating_sub(then) / 1000;
    if seconds < 60 {
        "now".to_owned()
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h", seconds / 3600)
    } else {
        format!("{}d", seconds / 86_400)
    }
}
