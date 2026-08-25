//! Where the prompt is typed.
//!
//! One card: a native editor to type in — text wraps, and the field grows with it — and under
//! it the thread's controls: the mode, the model, and the send or stop button, each menu
//! anchored over its own button. `<CR>` sends, `<S-CR>` breaks the line, and `<Esc>` hands the
//! keyboard to the timeline.
//!
//! `@`, `/` and `$` open an inline popdown over files, slash commands and skills, filtered by
//! what is typed after them and anchored at the trigger itself — the same shape the code
//! editor's completion takes. The anchor is measured with a hidden mirror: the same text, the
//! same width, and a marker where the trigger sits, so the popdown lands where the engine
//! wrapped the line.

use std::rc::Rc;

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::{component, view};

use zdt_icons::{self as icons, IconProps};

use crate::state::MenuKind;
use crate::use_agent;

/// What a trigger character asks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TriggerKind {
    /// `@`: a project file.
    File,
    /// `/`: a slash command.
    Command,
    /// `$`: a skill.
    Skill,
}

impl TriggerKind {
    /// The character that opens it.
    fn character(self) -> char {
        match self {
            Self::File => '@',
            Self::Command => '/',
            Self::Skill => '$',
        }
    }
}

/// One open trigger: its kind, and where its character sits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Trigger {
    kind: TriggerKind,
    /// The byte of the trigger character itself.
    start: usize,
}

/// One row the popdown offers.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Suggestion {
    /// What accepting writes over the trigger and the query.
    insert: String,
    /// What the row says.
    label: String,
    /// One line beside it, when there is one.
    description: String,
}

/// How many rows the popdown shows at most.
const MOST_SUGGESTED: usize = 8;

/// Where a suggest popdown goes: x, the mark's top and bottom, the window's height, and whether
/// there is room below.
type SuggestPlace = Option<(f64, f64, f64, f64, bool)>;

/// What taking a popdown row runs.
type Took = std::rc::Rc<dyn Fn(usize)>;

/// The composer.
#[component]
pub fn Composer(
    /// The editor element itself, for whatever gives it the keyboard.
    field: NodeRef,
) -> impl IntoView {
    let agent = use_agent();

    // The text, mirrored out of the element on every input. The element holds the truth; every
    // decision below is made against this mirror, so nothing depends on reading the caret back.
    let text: RwSignal<String, LocalStorage> = RwSignal::new_local(String::new());
    // The timer that lands the caret after a written value: the element takes a new value on
    // its own frame, and a selection set before that is clamped against the old text.
    let landing: Rc<std::cell::RefCell<Option<zgui::view::time::TimeoutHandle>>> =
        Rc::new(std::cell::RefCell::new(None));
    let trigger: RwSignal<Option<Trigger>, LocalStorage> = RwSignal::new_local(None);
    // A trigger key was pressed; the next input says where its character landed.
    let arming: RwSignal<Option<TriggerKind>, LocalStorage> = RwSignal::new_local(None);
    let pick_at: RwSignal<usize, LocalStorage> = RwSignal::new_local(0);
    let files: RwSignal<Rc<Vec<String>>, LocalStorage> = RwSignal::new_local(Rc::new(Vec::new()));

    // Drafts: the field follows the selection. Leaving a thread keeps what was typed; coming
    // back brings it out again, from the daemon, so it survives an editor restart too.
    let holding: RwSignal<Option<zdt_agent::thread::ThreadId>, LocalStorage> =
        RwSignal::new_local(None);
    let keeping = {
        let agent = agent.clone();
        zgui::reactive::RenderEffect::new(move |_| {
            let fresh = agent.selected();
            let held = holding.get_untracked();
            if fresh == held {
                return;
            }
            if let Some(old) = held {
                agent.save_draft(old, text.get_untracked());
            }
            let draft = fresh
                .and_then(|id| agent.client().thread(id))
                .map(|shell| shell.draft)
                .unwrap_or_default();
            field.set_value(&draft);
            text.set(draft);
            trigger.set(None);
            holding.set(fresh);
        })
    };
    on_cleanup_local(move || drop(keeping));

    // The debounced save: a pause in typing writes the draft down.
    let saving: Rc<std::cell::RefCell<Option<zgui::view::time::TimeoutHandle>>> =
        Rc::new(std::cell::RefCell::new(None));
    let keep_soon = {
        let agent = agent.clone();
        let saving = Rc::clone(&saving);
        move || {
            if zgui::view::time::Timers::current().is_none() {
                return;
            }
            let agent = agent.clone();
            let hold =
                zgui::view::time::set_timeout(std::time::Duration::from_millis(1200), move || {
                    if let Some(thread) = holding.get_untracked() {
                        agent.save_draft(thread, text.get_untracked());
                    }
                });
            *saving.borrow_mut() = Some(hold);
        }
    };

    // The trigger's position comes from the text itself: the armed character is found where the
    // old and the new text part ways. Nothing here reads the element's selection, which not
    // every platform answers in step with the keys.
    let on_input = {
        let agent = agent.clone();
        let keep_soon = keep_soon.clone();
        move |cx: &mut EventCx<'_, events::Input>| {
            let Some(payload) = cx.payload().as_value() else {
                return;
            };
            let fresh = payload.value.to_string();
            let old = text.get_untracked();
            if let Some(kind) = arming.get_untracked() {
                arming.set(None);
                if let Some(start) = landed_trigger(&old, &fresh, kind) {
                    trigger.set(Some(Trigger { kind, start }));
                    pick_at.set(0);
                    if kind == TriggerKind::File {
                        fetch_files(&agent, files);
                    }
                }
            }
            text.set(fresh);
            keep_soon();
        }
    };

    // What stands between the trigger character and the next whitespace or the text's end.
    let query = move || {
        let held = trigger.get()?;
        text.with(|text| {
            let rest = text.get(held.start..)?;
            let mut chars = rest.chars();
            if chars.next() != Some(held.kind.character()) {
                return None;
            }
            let after = held.start + held.kind.character().len_utf8();
            let tail = &text[after..];
            // A space straight after the trigger is the person moving on.
            if tail.chars().next().is_some_and(char::is_whitespace) {
                return None;
            }
            let end = tail
                .find(char::is_whitespace)
                .map_or(text.len(), |at| after + at);
            Some(text[after..end].to_owned())
        })
    };

    // A trigger whose character is gone, or with a space typed straight after it, closes itself.
    let closing = zgui::reactive::RenderEffect::new(move |_| {
        if trigger.get().is_some() && query().is_none() {
            trigger.set(None);
        }
    });
    on_cleanup_local(move || drop(closing));

    // The rows under the caret's query, derived once and read from a signal: the view tracks
    // it, and the keys read it without waking anything.
    let rows: RwSignal<Vec<Suggestion>, LocalStorage> = RwSignal::new_local(Vec::new());
    let deriving = {
        let agent = agent.clone();
        zgui::reactive::RenderEffect::new(move |_| {
            let fresh = (|| {
                let held = trigger.get()?;
                let typed = query()?;
                let catalog = agent.client().catalog();
                let all: Vec<Suggestion> = match held.kind {
                    TriggerKind::File => files
                        .get()
                        .iter()
                        .map(|path| Suggestion {
                            insert: format!("@{path} "),
                            label: path.clone(),
                            description: String::new(),
                        })
                        .collect(),
                    TriggerKind::Command => catalog
                        .commands
                        .iter()
                        .map(|command| Suggestion {
                            insert: format!("/{} ", command.name),
                            label: format!("/{}", command.name),
                            description: clip_line(&command.description, 88),
                        })
                        .collect(),
                    // A skill is carried out by its slash command; `$` only narrows the list.
                    TriggerKind::Skill => catalog
                        .skills
                        .iter()
                        .map(|name| Suggestion {
                            insert: format!("/{name} "),
                            label: format!("${name}"),
                            description: String::new(),
                        })
                        .collect(),
                };
                let labels: Vec<String> = all.iter().map(|row| row.label.clone()).collect();
                let ranked = zdt_core::search::fuzzy::rank(&labels, &typed, MOST_SUGGESTED);
                Some(
                    ranked
                        .into_iter()
                        .map(|found| all[found.index].clone())
                        .collect::<Vec<Suggestion>>(),
                )
            })()
            .unwrap_or_default();
            if rows.with_untracked(|held| *held != fresh) {
                rows.set(fresh);
            }
        })
    };
    on_cleanup_local(move || drop(deriving));

    // Accepting writes the trigger and its query over with the row's text.
    let accept = {
        let landing = Rc::clone(&landing);
        move |index: usize| {
            let Some(held) = trigger.get_untracked() else {
                return;
            };
            let Some(row) = rows.with_untracked(|rows| rows.get(index).cloned()) else {
                return;
            };
            let whole = text.get_untracked();
            if held.start >= whole.len() {
                return;
            }
            let after = held.start + held.kind.character().len_utf8();
            let end = whole[after..]
                .find(char::is_whitespace)
                .map_or(whole.len(), |at| after + at);
            let written = format!("{}{}{}", &whole[..held.start], row.insert, &whole[end..]);
            let landed = held.start + row.insert.len();
            field.set_value(&written);
            text.set(written);
            trigger.set(None);
            // The caret, once the value has taken: timers run at a frame's start, after the
            // edit model has applied what was written.
            if zgui::view::time::Timers::current().is_some() {
                let hold = zgui::view::time::set_timeout(
                    std::time::Duration::from_millis(30),
                    move || {
                        field.set_selection(landed..landed);
                    },
                );
                *landing.borrow_mut() = Some(hold);
            }
        }
    };

    let clear = {
        let landing = Rc::clone(&landing);
        move || {
            landing.borrow_mut().take();
            field.set_value("");
            text.set(String::new());
            trigger.set(None);
            arming.set(None);
        }
    };

    let send = {
        let agent = agent.clone();
        move || {
            let whole = text.get_untracked();
            if whole.trim().is_empty() {
                // An empty send takes the proposed plan, when one waits.
                if agent.client().plan().is_some() {
                    agent.implement();
                }
                return;
            }
            agent.send(whole);
            clear();
        }
    };

    let on_key = {
        let (agent, send, accept) = (agent.clone(), send.clone(), accept.clone());
        move |cx: &mut EventCx<'_, events::KeyDown>| {
            use zgui::vocab::{Key, NamedKey};
            let key = cx.key.clone();
            let modifiers = cx.modifiers;

            // An open menu answers first.
            if agent.menu_open() {
                match &key {
                    Key::Named(NamedKey::ArrowDown) => {
                        agent.menu_step(1);
                        return finish(cx);
                    }
                    Key::Named(NamedKey::ArrowUp) => {
                        agent.menu_step(-1);
                        return finish(cx);
                    }
                    Key::Named(NamedKey::Enter) => {
                        agent.menu_take();
                        return finish(cx);
                    }
                    Key::Named(NamedKey::Escape) => {
                        agent.close_menu();
                        return finish(cx);
                    }
                    _ => {
                        agent.close_menu();
                    }
                }
            }

            // Then the popdown.
            if trigger.get_untracked().is_some() {
                match &key {
                    Key::Named(NamedKey::Escape) => {
                        trigger.set(None);
                        return finish(cx);
                    }
                    Key::Named(NamedKey::ArrowDown) => {
                        step_pick(pick_at, rows, 1);
                        return finish(cx);
                    }
                    Key::Named(NamedKey::ArrowUp) => {
                        step_pick(pick_at, rows, -1);
                        return finish(cx);
                    }
                    Key::Character(typed) if modifiers.control() && typed.as_str() == "n" => {
                        step_pick(pick_at, rows, 1);
                        return finish(cx);
                    }
                    Key::Character(typed) if modifiers.control() && typed.as_str() == "p" => {
                        step_pick(pick_at, rows, -1);
                        return finish(cx);
                    }
                    Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Tab) => {
                        accept(pick_at.get_untracked());
                        return finish(cx);
                    }
                    _ => {}
                }
            }

            match &key {
                Key::Named(NamedKey::Enter) if !modifiers.shift() => {
                    send();
                    finish(cx);
                }
                Key::Named(NamedKey::Escape) => {
                    agent.to_list();
                    finish(cx);
                }
                // The trigger is only armed here; the input that follows says where its
                // character landed, and whether it stands where a mention may begin.
                Key::Character(typed) if typed.as_str() == "@" => {
                    arming.set(Some(TriggerKind::File));
                }
                Key::Character(typed)
                    if typed.as_str() == "/" && text.with_untracked(String::is_empty) =>
                {
                    arming.set(Some(TriggerKind::Command));
                }
                Key::Character(typed) if typed.as_str() == "$" => {
                    arming.set(Some(TriggerKind::Skill));
                }
                _ => {}
            }
        }
    };

    let taken = {
        let agent = agent.clone();
        move |_: &mut EventCx<'_, events::FocusIn>| {
            agent.composer_focused();
        }
    };
    // A popdown is about the word being typed; the keyboard leaving ends both.
    let left_field = move |_: &mut EventCx<'_, events::FocusOut>| {
        trigger.set(None);
    };

    view! {
        box(class = "agent-composer", on:focus_in = taken) {
            column(class = "agent-composer__card") {
                box(class = "agent-composer__editwrap") {
                    editor(
                        class = "agent-composer__editor",
                        node_ref = field,
                        tabindex = Focus::Programmatic,
                        a11y:role = Role::TextInput,
                        a11y:label = "Prompt",
                        on:input = on_input,
                        on:key_down = on_key,
                        on:focus_out = left_field
                    ) {}
                    Anchored(
                        text = text,
                        trigger = trigger,
                        rows = rows,
                        at = pick_at,
                        took = std::rc::Rc::new(accept),
                        field = field
                    )
                }
                Foot(text = text, send = std::rc::Rc::new(send))
            }
        }
    }
}

/// Uses a key up: the element's own editing must not also act on it.
fn finish(cx: &mut EventCx<'_, events::KeyDown>) {
    cx.prevent_default();
    cx.stop_propagation();
}

/// Moves the popdown caret, wrapping over however many rows there are.
fn step_pick(
    pick_at: RwSignal<usize, LocalStorage>,
    rows: RwSignal<Vec<Suggestion>, LocalStorage>,
    delta: isize,
) {
    let count = rows.with_untracked(Vec::len);
    if count == 0 {
        return;
    }
    pick_at.update(|held| {
        *held = (*held as isize + delta).rem_euclid(count as isize) as usize;
    });
}

/// Where an armed trigger's character landed, when it did and where one may begin.
///
/// The old and the new text part ways at the insertion; the character there must be the
/// trigger's own, standing at the start or after whitespace. A slash command stands only at
/// the very start.
fn landed_trigger(old: &str, fresh: &str, kind: TriggerKind) -> Option<usize> {
    let start = old
        .as_bytes()
        .iter()
        .zip(fresh.as_bytes())
        .position(|(was, is)| was != is)
        .unwrap_or(old.len().min(fresh.len()));
    if !fresh.is_char_boundary(start) || !fresh[start..].starts_with(kind.character()) {
        return None;
    }
    match kind {
        TriggerKind::Command => (start == 0).then_some(0),
        TriggerKind::File | TriggerKind::Skill => (start == 0
            || fresh[..start]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace))
        .then_some(start),
    }
}

/// Walks the project once per opening, into the popdown's list.
fn fetch_files(agent: &crate::AgentUi, files: RwSignal<Rc<Vec<String>>, LocalStorage>) {
    agent.host().files(Rc::new(move |walked: Vec<String>| {
        files.set(Rc::new(walked));
    }));
}

/// The popdown, measured onto the trigger.
///
/// A hidden mirror repeats the text up to the trigger inside a box the editor's exact width,
/// with a marker run where the trigger sits; the engine wraps both the same way, so the
/// marker's box is where the trigger is drawn. The popdown opens under that point when the
/// window has room below, and over it otherwise.
// The list macro takes a closure by construction, so the one it is handed here is not redundant.
#[allow(clippy::redundant_closure)]
#[component]
fn Anchored(
    /// The whole text.
    text: RwSignal<String, LocalStorage>,
    /// The open trigger.
    trigger: RwSignal<Option<Trigger>, LocalStorage>,
    /// The rows to offer. Empty means closed.
    rows: RwSignal<Vec<Suggestion>, LocalStorage>,
    /// Which row the caret is on.
    at: RwSignal<usize, LocalStorage>,
    /// Takes the row at an index.
    took: Took,
    /// The editor the mirror repeats.
    field: NodeRef,
) -> impl IntoView {
    let marker = NodeRef::new();
    let measured = marker.observe_border_box();

    let before = move || {
        let Some(held) = trigger.get() else {
            return String::new();
        };
        text.with(|text| {
            if held.start <= text.len() && text.is_char_boundary(held.start) {
                text[..held.start].to_owned()
            } else {
                String::new()
            }
        })
    };

    // Where the popdown's corner goes, in window coordinates: the marker box is observed there.
    let compute_place = move || {
        let mark = measured.get()?;
        let root = field.window_root()?;
        let window = root.bounds()?;
        let scale = f64::from(field.scale());
        // An empty run can measure without height; a line of the wrap's text stands in.
        let line = if mark.size.height.0 > 1.0 {
            f64::from(mark.size.height.0)
        } else {
            18.0 * scale
        };
        let width = f64::from(window.size.width.0) / scale;
        let height = f64::from(window.size.height.0) / scale;
        let x = (f64::from(mark.origin.x.0 - window.origin.x.0) / scale)
            .clamp(0.0, (width - 340.0).max(0.0));
        let top = f64::from(mark.origin.y.0 - window.origin.y.0) / scale;
        let bottom = top + line / scale;
        let need = (rows.with(Vec::len) as f64) * 24.0 + 10.0;
        Some((x, top, bottom, height, height - bottom >= need))
    };
    let placed: RwSignal<SuggestPlace, LocalStorage> = RwSignal::new_local(None);
    let placing = zgui::reactive::RenderEffect::new(move |_| {
        let fresh = compute_place();
        if placed.get_untracked() != fresh {
            placed.set(fresh);
        }
    });
    on_cleanup_local(move || drop(placing));

    // The popdown stays mounted, on the overlay band over everything the window draws, and is
    // shown or hidden by display alone: swapping the subtree in and out leaves its last paint
    // behind, and anything under the editor's own layer is painted through.
    let shown = move || {
        (trigger.get().is_none() || rows.with(Vec::is_empty) || placed.get().is_none())
            .then(|| "none".to_owned())
    };
    let left = move || placed.get().map(|(x, ..)| format!("{x:.0}px"));
    let top_css = move || {
        placed.get().and_then(|(_, _, bottom, _, fits_below)| {
            fits_below.then(|| format!("{:.0}px", bottom + 2.0))
        })
    };
    let bottom_css = move || {
        placed.get().and_then(|(_, top, _, height, fits_below)| {
            (!fits_below).then(|| format!("{:.0}px", height - top + 2.0))
        })
    };
    let took_held: RwSignal<Option<Took>, LocalStorage> = RwSignal::new_local(Some(took));
    // Only how many: the rows themselves read their content off the signal, so a list that
    // narrows repaints every row it keeps.
    let indexes = move || {
        if trigger.get().is_none() {
            return Vec::new();
        }
        (0..rows.with(Vec::len)).collect::<Vec<usize>>()
    };

    let before_shown = move || trigger.get().is_none().then(|| "none".to_owned());

    view! {
        box(class = "agent-composer__mirror", style:display = before_shown) {
            text {{before}}
            text(node_ref = marker, class = "agent-composer__mark") {"\u{200b}"}
        }
        Portal {
            column(
                class = "agent-pop agent-pop--suggest",
                style:display = shown,
                style:left = left,
                style:top = top_css,
                style:bottom = bottom_css
            ) {
                for index in move || indexes(), key = |index: &usize| *index {
                    SuggestRowView(index = index, rows = rows, at = at, took = took_held)
                }
            }
        }
    }
}

/// One popdown row.
///
/// Everything it draws is read through the shared signals: the row is keyed by its place in the
/// list, and its content, like its highlight, changes under it as the query narrows.
#[component]
fn SuggestRowView(
    /// Which place in the list this row holds.
    index: usize,
    /// Every row's content.
    rows: RwSignal<Vec<Suggestion>, LocalStorage>,
    /// Which place the caret is on.
    at: RwSignal<usize, LocalStorage>,
    /// Takes the row at an index.
    took: RwSignal<Option<Took>, LocalStorage>,
) -> impl IntoView {
    let pick = move |event: &mut EventCx<'_, events::PointerDown>| {
        event.stop_propagation();
        if let Some(took) = took.get_untracked() {
            took(index);
        }
    };
    let lit = move || {
        (rows.with(|rows| at.get().min(rows.len().saturating_sub(1))) == index)
            .then(|| "true".to_owned())
    };
    let label_text = move || {
        rows.with(|rows| rows.get(index).map(|row| row.label.clone()))
            .unwrap_or_default()
    };
    let description = move || {
        rows.with(|rows| rows.get(index).map(|row| row.description.clone()))
            .unwrap_or_default()
    };

    view! {
        row(class = "agent-pop__row", attr:data-on = lit, on:pointer_down = pick) {
            label(class = "agent-pop__label nowrap") {{label_text}}
            label(class = "agent-pop__desc muted nowrap") {{description}}
        }
    }
}

/// At most `most` characters of one line.
fn clip_line(text: &str, most: usize) -> String {
    let line = text.lines().next().unwrap_or_default();
    if line.chars().count() <= most {
        return line.to_owned();
    }
    let cut: String = line.chars().take(most).collect();
    format!("{cut}\u{2026}")
}

/// The mode and model menus, each over its own button.
// The list macro takes a closure by construction, so the one it is handed here is not redundant.
#[allow(clippy::redundant_closure)]
#[component]
fn MenuPop(
    /// Which menu this popover belongs to.
    kind: MenuKind,
    /// The button the menu opens over.
    seat: NodeRef,
) -> impl IntoView {
    let agent = use_agent();

    // Directly over the button, in window coordinates, on the overlay band.
    let compute_place = {
        let agent = agent.clone();
        move || {
            if agent.menu().map(|(held, _)| held) != Some(kind) {
                return None;
            }
            let chip = seat.window_bounds()?;
            let root = seat.window_root()?;
            let window = root.bounds()?;
            let scale = f64::from(seat.scale());
            let width = f64::from(window.size.width.0) / scale;
            let height = f64::from(window.size.height.0) / scale;
            let x = (f64::from(chip.origin.x.0 - window.origin.x.0) / scale)
                .clamp(0.0, (width - 320.0).max(0.0));
            let bottom = height - f64::from(chip.origin.y.0 - window.origin.y.0) / scale + 6.0;
            Some((x, bottom))
        }
    };
    let placed: RwSignal<Option<(f64, f64)>, LocalStorage> = RwSignal::new_local(None);
    let placing = zgui::reactive::RenderEffect::new(move |_| {
        let fresh = compute_place();
        if placed.get_untracked() != fresh {
            placed.set(fresh);
        }
    });
    on_cleanup_local(move || drop(placing));

    // Mounted for good and shown by display alone: swapping the subtree in and out leaves its
    // last paint behind.
    let shown = move || placed.get().is_none().then(|| "none".to_owned());
    let left = move || placed.get().map(|(x, _)| format!("{x:.0}px"));
    let bottom_css = move || placed.get().map(|(_, bottom)| format!("{bottom:.0}px"));
    let held_agent: RwSignal<Option<crate::AgentUi>, LocalStorage> =
        RwSignal::new_local(Some(agent.clone()));
    // Only how many: each row reads its own content and highlight off the state, so the caret
    // moving repaints the rows it left and reached.
    let indexes = move || {
        let Some(agent) = held_agent.get_untracked() else {
            return Vec::new();
        };
        if agent.menu().map(|(held, _)| held) != Some(kind) {
            return Vec::new();
        }
        (0..agent.menu_rows(kind).len()).collect::<Vec<usize>>()
    };

    let dismiss = move |event: &mut EventCx<'_, events::PointerDown>| {
        event.stop_propagation();
        if let Some(agent) = held_agent.get_untracked() {
            agent.close_menu();
        }
    };
    let backdrop_shown = shown;

    view! {
        Portal {
            box(
                class = "agent-pop__backdrop",
                style:display = backdrop_shown,
                on:pointer_down = dismiss
            ) {}
            column(
                class = "agent-pop agent-pop--menu",
                style:display = shown,
                style:left = left,
                style:bottom = bottom_css
            ) {
                for index in move || indexes(), key = |index: &usize| *index {
                    MenuRowView(kind = kind, index = index)
                }
            }
        }
    }
}

/// One menu row.
///
/// Everything it draws is read through the state: the row is keyed by its place, and the
/// highlight follows the menu's caret while the row stands.
#[component]
fn MenuRowView(
    /// Which menu the row belongs to.
    kind: MenuKind,
    /// Which place in the menu this row holds.
    index: usize,
) -> impl IntoView {
    let agent = use_agent();

    let pick = {
        let agent = agent.clone();
        move |event: &mut EventCx<'_, events::PointerDown>| {
            event.stop_propagation();
            agent.menu_choose(kind, index);
        }
    };
    let row = {
        let agent = agent.clone();
        move || agent.menu_rows(kind).into_iter().nth(index)
    };
    // Model rows carry the provider's mark, the way t3code's picker names its makers.
    let mark = {
        let agent = agent.clone();
        move || {
            if kind == MenuKind::Model {
                agent.provider_mark()
            } else {
                None
            }
        }
    };
    let mark_icon = {
        let mark = mark.clone();
        move || mark().unwrap_or(icons::DOT)
    };
    let mark_shown = move || mark().is_none().then(|| "none".to_owned());
    let brand_class = "icon--xs icon--brand agent-pop__brand";
    let lit = {
        let agent = agent.clone();
        move || (agent.menu() == Some((kind, index))).then(|| "true".to_owned())
    };
    let label_text = {
        let row = row.clone();
        move || row().map(|row| row.label).unwrap_or_default()
    };
    let description = {
        let row = row.clone();
        move || row().map(|row| row.description).unwrap_or_default()
    };
    let dot = move || (!row().is_some_and(|row| row.current)).then(|| "none".to_owned());

    view! {
        row(class = "agent-pop__row agent-pop__row--tall", attr:data-on = lit, on:pointer_down = pick) {
            Icon(
                icon = Signal::derive_local(mark_icon),
                class = brand_class,
                style:display = mark_shown
            )
            column(class = "agent-pop__lines") {
                label(class = "agent-pop__label nowrap") {{label_text}}
                label(class = "agent-pop__desc muted nowrap") {{description}}
            }
            box(class = "fill") {}
            Icon(icon = icons::CIRCLE_CHECK, class = "icon--xs agent-pop__check", style:display = dot)
        }
    }
}

/// The strip under the editor: mode, model, and the send or stop button.
#[component]
fn Foot(
    /// The composer's text, for the send button.
    text: RwSignal<String, LocalStorage>,
    /// Sends it.
    send: std::rc::Rc<dyn Fn()>,
) -> impl IntoView {
    let agent = use_agent();

    let mode = {
        let agent = agent.clone();
        move || {
            agent
                .selected_shell()
                .map(|shell| shell.mode.label().to_owned())
                .unwrap_or_default()
        }
    };
    let model = {
        let agent = agent.clone();
        move || {
            agent
                .menu_rows(MenuKind::Model)
                .into_iter()
                .find(|row| row.current)
                .map_or_else(|| "Default".to_owned(), |row| row.label)
        }
    };
    // The chip names the level, and the default level names the chip: a second "Default"
    // beside the model's would say nothing.
    let effort = {
        let agent = agent.clone();
        move || {
            agent
                .menu_rows(MenuKind::Effort)
                .into_iter()
                .find(|row| row.current)
                .map_or_else(|| "Effort".to_owned(), |row| row.label)
                .replace("Default", "Effort")
        }
    };
    // The chip only exists once the session has said what levels it takes.
    let effort_hidden = {
        let agent = agent.clone();
        move || (!agent.efforts_known()).then(|| "none".to_owned())
    };
    // The provider's mark on the model chip: which maker answers, before which model. A brand
    // mark is filled art and the chevron is stroked, so each is its own element.
    let chip_mark = {
        let agent = agent.clone();
        move || agent.provider_mark()
    };
    let chip_mark_icon = {
        let chip_mark = chip_mark.clone();
        move || chip_mark().unwrap_or(icons::DOT)
    };
    let chip_mark_shown = {
        let chip_mark = chip_mark.clone();
        move || chip_mark().is_none().then(|| "none".to_owned())
    };
    let chip_chevron_shown = move || chip_mark().is_some().then(|| "none".to_owned());
    let busy = {
        let agent = agent.clone();
        move || {
            agent
                .selected_shell()
                .is_some_and(|shell| shell.state.is_busy())
        }
    };
    let stoppable = {
        let busy = busy.clone();
        move || (!busy()).then(|| "none".to_owned())
    };
    let sendable = {
        let busy = busy.clone();
        move || busy().then(|| "none".to_owned())
    };
    let dimmed = {
        let agent = agent.clone();
        move || {
            let empty = text.with(|text| text.trim().is_empty());
            (empty && agent.client().plan().is_none()).then(|| "true".to_owned())
        }
    };

    let open_mode = {
        let agent = agent.clone();
        move |event: &mut EventCx<'_, events::PointerDown>| {
            event.stop_propagation();
            if !agent.close_menu() {
                agent.open_menu(MenuKind::Mode);
            }
        }
    };
    let open_model = {
        let agent = agent.clone();
        move |event: &mut EventCx<'_, events::PointerDown>| {
            event.stop_propagation();
            if !agent.close_menu() {
                agent.open_menu(MenuKind::Model);
            }
        }
    };
    let open_effort = {
        let agent = agent.clone();
        move |event: &mut EventCx<'_, events::PointerDown>| {
            event.stop_propagation();
            if !agent.close_menu() {
                agent.open_menu(MenuKind::Effort);
            }
        }
    };

    // The context ring: how much of the window the conversation has used. Hidden until the
    // provider has said what the window is.
    let used = {
        let agent = agent.clone();
        move || {
            agent
                .selected_shell()
                .map(|shell| shell.usage)
                .filter(|usage| usage.context_limit > 0)
                .map(|usage| {
                    (usage.context_tokens as f64 / usage.context_limit as f64).clamp(0.0, 1.0)
                })
        }
    };
    let ring_hidden = {
        let used = used.clone();
        move || used().is_none().then(|| "none".to_owned())
    };
    let ring_svg = {
        let used = used.clone();
        move || zgui::vocab::PropValue::from(ring_art(used().unwrap_or(0.0)))
    };
    // Amber past four fifths: the window running out is worth a glance.
    let ring_tone = {
        let used = used.clone();
        move || (used().unwrap_or(0.0) >= 0.8).then(|| "true".to_owned())
    };
    let ring_label = {
        let used = used.clone();
        move || format!("Context {:.0}% used", used().unwrap_or(0.0) * 100.0)
    };
    let stop = {
        let agent = agent.clone();
        move |event: &mut EventCx<'_, events::PointerDown>| {
            event.stop_propagation();
            agent.interrupt();
        }
    };
    let sending = move |event: &mut EventCx<'_, events::PointerDown>| {
        event.stop_propagation();
        send();
    };

    let mode_seat = NodeRef::new();
    let model_seat = NodeRef::new();
    let effort_seat = NodeRef::new();

    view! {
        row(class = "agent-composer__foot") {
            control(
                class = "agent-composer__chip",
                node_ref = model_seat,
                tabindex = Focus::Programmatic,
                a11y:label = "Choose the model",
                on:pointer_down = open_model
            ) {
                Icon(
                    icon = Signal::derive_local(chip_mark_icon),
                    class = "icon--xs icon--brand",
                    style:display = chip_mark_shown
                )
                Icon(icon = icons::CHEVRON_UP, class = "icon--xs", style:display = chip_chevron_shown)
                label(class = "nowrap") {{model}}
            }
            MenuPop(kind = MenuKind::Model, seat = model_seat)
            control(
                class = "agent-composer__chip",
                node_ref = mode_seat,
                tabindex = Focus::Programmatic,
                a11y:label = "Choose the mode",
                on:pointer_down = open_mode
            ) {
                Icon(icon = icons::CHEVRON_UP, class = "icon--xs")
                label(class = "nowrap") {{mode}}
            }
            MenuPop(kind = MenuKind::Mode, seat = mode_seat)
            control(
                class = "agent-composer__chip",
                node_ref = effort_seat,
                tabindex = Focus::Programmatic,
                a11y:label = "Choose the effort",
                style:display = effort_hidden,
                on:pointer_down = open_effort
            ) {
                Icon(icon = icons::CHEVRON_UP, class = "icon--xs")
                label(class = "nowrap") {{effort}}
            }
            MenuPop(kind = MenuKind::Effort, seat = effort_seat)
            box(class = "fill") {}
            vector(
                class = "agent-composer__ring",
                prop:svg = ring_svg,
                style:display = ring_hidden,
                attr:data-high = ring_tone,
                a11y:role = Role::Image,
                a11y:label = ring_label()
            ) {}
            control(
                class = "agent-composer__stop",
                tabindex = Focus::Programmatic,
                a11y:label = "Stop the turn",
                style:display = stoppable,
                on:pointer_down = stop
            ) {
                Icon(icon = icons::SQUARE, class = "icon--xs")
            }
            control(
                class = "agent-composer__send",
                tabindex = Focus::Programmatic,
                a11y:label = "Send",
                attr:data-dim = dimmed,
                style:display = sendable,
                on:pointer_down = sending
            ) {
                Icon(icon = icons::SEND_HORIZONTAL, class = "icon--xs")
            }
        }
    }
}

/// The ring, drawn for a used fraction of the window.
///
/// A quiet full circle behind a stroked arc from twelve o'clock. Both name their own stroke, so
/// the icon class's inherited one touches neither.
fn ring_art(fraction: f64) -> String {
    let fraction = fraction.clamp(0.0, 1.0);
    let arc = if fraction >= 0.999 {
        // A closed arc degenerates; the full window is a second circle.
        r#"<circle cx="10" cy="10" r="7" stroke="currentColor" stroke-width="2.6" fill="none"/>"#
            .to_owned()
    } else if fraction <= 0.002 {
        String::new()
    } else {
        let angle = fraction * std::f64::consts::TAU;
        let x = 10.0 + 7.0 * angle.sin();
        let y = 10.0 - 7.0 * angle.cos();
        let large = i32::from(fraction > 0.5);
        format!(
            r#"<path d="M 10 3 A 7 7 0 {large} 1 {x:.2} {y:.2}" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" fill="none"/>"#,
        )
    };
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20" fill="none"><circle cx="10" cy="10" r="7" stroke="currentColor" stroke-width="2.6" opacity="0.22" fill="none"/>{arc}</svg>"#,
    )
}
