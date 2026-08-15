//! The picker, drawn.
//!
//! A prompt across the top, the matches down the left, and what the caret is on shown on the right.
//!
//! # The preview
//!
//! One editor, mounted once and kept for as long as the picker is open, whose text is replaced as
//! the caret moves. Not one per row and not rebuilt per selection: an editor costs a syntax worker
//! and a first parse, and paying that twenty times while somebody holds `<C-j>` is the difference
//! between a picker that keeps up and one that does not.
//!
//! The read is debounced for the same reason. Walking a list is not a request to read every file
//! it passes over — only the one it stops on.

use std::time::Duration;

use zgui::prelude::*;
use zgui::{component, view};
use zgui_editor::EditorProps;
use zgui_ui::prelude::*;

use crate::picker::{Row, use_picker};

/// How tall one row is, which the list is told rather than measuring.
const ROW: f32 = 22.0;

/// How long the caret has to rest on a row before its file is read.
const PREVIEW_DEBOUNCE: Duration = Duration::from_millis(40);

/// The layer the previewed match is banded in.
const MATCH_LAYER: &str = "picker-match";

/// Puts the previewed file at the place the row stands for, and picks the match out.
///
/// Centred rather than merely visible: a hit at the bottom of the preview with nothing under it
/// reads as the end of the file even when it is not.
fn show_place(handle: &zgui_editor::EditorHandle, preview: &crate::picker::Preview) {
    let Some(line) = preview.line else {
        handle.clear_decorations(MATCH_LAYER);
        handle.command(zgui_editor::Command::Scroll(
            zgui_editor::ScrollCmd::ToLine(0),
        ));
        return;
    };

    let (at, matched) = handle.query(|snapshot| {
        let rope = snapshot.rope();
        let line = (line as usize)
            .saturating_sub(1)
            .min(rope.len_lines().saturating_sub(1));
        let start = rope.char_to_byte(rope.line_to_char(line));
        // The match is a range within the line, which is where the searcher measured it.
        let matched = preview.matched.as_ref().map(|range| {
            let end = rope.len_bytes();
            (start + range.start).min(end)..(start + range.end).min(end)
        });
        (start, matched)
    });

    handle.command(zgui_editor::Command::SetSelections {
        selections: vec![zgui_editor::Selection::caret(at)],
        primary: 0,
    });
    handle.command(zgui_editor::Command::Scroll(
        zgui_editor::ScrollCmd::CursorCenter,
    ));

    match matched.filter(|range| !range.is_empty()) {
        Some(range) => handle.set_decorations(
            MATCH_LAYER,
            vec![zgui_editor::Decoration {
                range,
                kind: zgui_editor::DecorationKind::Background(
                    zgui_editor::decoration::Paint::Property("editor-search-current".into()),
                ),
            }],
        ),
        None => handle.clear_decorations(MATCH_LAYER),
    }
}

/// How much of a file is worth previewing.
///
/// Only the head is ever on the screen, and reading a hundred megabytes to show forty lines of it
/// is a stall for nothing.
const PREVIEW_HEAD: u64 = 256 * 1024;

/// The modal.
#[component]
pub fn Picker() -> impl IntoView {
    let picker = use_picker();

    view! {
        {move || {
            use crate::ui::Erase;
            match picker.source() {
                Some(source) => view! { Open(title = source.title(), previews = source.previews()) }
                    .any(),
                None => ().any(),
            }
        }}
    }
}

/// One picker, for as long as it is open.
///
/// Built fresh per opening rather than kept and hidden, because the preview editor holds a
/// document and a syntax worker that a closed picker has no use for.
#[component]
fn Open(
    /// What the picker calls itself.
    title: &'static str,
    /// Whether to show what the caret is on beside the list.
    previews: bool,
) -> impl IntoView {
    let picker = use_picker();
    let field = NodeRef::new();
    let query = RwSignal::new_local(picker.query());

    // From a timer, because a node that is not mounted cannot take focus.
    let claim = zgui::view::time::Timers::current()
        .map(|timers| timers.set_timeout(Duration::ZERO, move || field.focus()));
    on_cleanup_local(move || drop(claim));

    // What is typed reaches the picker through here rather than through the field's own binding,
    // so that the search starts on the keystroke rather than on the frame after it.
    let typing = {
        let picker = picker.clone();
        zgui::reactive::RenderEffect::new(move |_| {
            picker.set_query(&query.get());
        })
    };
    on_cleanup_local(move || drop(typing));

    let on_key = {
        let picker = picker.clone();
        move |event: &mut EventCx<'_, events::KeyDown>| {
            if handle(&picker, event) {
                event.prevent_default();
                event.stop_propagation();
            } else {
                // Everything else is text for the field, and must not reach the editor behind.
                event.stop_propagation();
            }
        }
    };

    let counts = {
        let picker = picker.clone();
        move || {
            let (matched, total) = picker.counts();
            if total == 0 {
                String::new()
            } else {
                format!("{matched}/{total}")
            }
        }
    };
    let working = {
        let picker = picker.clone();
        move || picker.is_working().then(|| "true".to_owned())
    };

    view! {
        box(class = "picker__scrim", on:pointer_down = {
            let picker = picker.clone();
            move |_| picker.close()
        }) {}

        column(
            class = "picker",
            attr:data-preview = previews.then(|| "true".to_owned()),
            a11y:role = Role::Dialog,
            a11y:label = title,
            on:key_down = on_key
        ) {
            row(class = "picker__prompt") {
                label(class = "picker__title nowrap") {{title}}
                Input(
                    class = "picker__input",
                    node_ref = field,
                    value = Binding::from(query),
                    a11y:label = title,
                )
                label(class = "picker__counts nowrap", attr:data-working = working) {{counts}}
            }

            row(class = "picker__body") {
                Matches()
                Preview(shown = previews)
            }
        }
    }
}

/// What the keys do while a picker is open.
///
/// Answers whether the key was one of them. These are the picker's own, not the keymap's: a picker
/// is a text field first, and a keymap row that took `j` would make it one nobody could type in.
fn handle(picker: &crate::picker::Picker, event: &EventCx<'_, events::KeyDown>) -> bool {
    let control = event.modifiers.control();
    match &event.key {
        Key::Named(NamedKey::Escape) => picker.close(),
        Key::Named(NamedKey::Enter) => picker.activate(),
        Key::Named(NamedKey::ArrowDown) => picker.move_by(1),
        Key::Named(NamedKey::ArrowUp) => picker.move_by(-1),
        Key::Named(NamedKey::PageDown) => picker.move_by(10),
        Key::Named(NamedKey::PageUp) => picker.move_by(-10),
        Key::Character(text) if control => match text.as_str() {
            "j" | "n" => picker.move_by(1),
            "k" | "p" => picker.move_by(-1),
            "d" => picker.move_by(10),
            "u" => picker.move_by(-10),
            "c" => picker.close(),
            _ => return false,
        },
        _ => return false,
    }
    true
}

/// The matches, down the left.
#[component]
fn Matches() -> impl IntoView {
    let picker = use_picker();

    let count = {
        let picker = picker.clone();
        move || picker.len()
    };
    let row = move |index: usize| view! { MatchRow(index = index) };

    view! {
        VirtualList(
            class = "picker__list",
            count = Signal::derive_local(count),
            row_size = ROW,
            row = row,
            label = "Matches",
        )
    }
}

/// One match.
#[component]
fn MatchRow(
    /// Where it is in the list.
    index: usize,
) -> impl IntoView {
    let picker = use_picker();

    let row = {
        let picker = picker.clone();
        move || picker.rows().get(index).cloned()
    };
    let selected = {
        let picker = picker.clone();
        move || (picker.at() == index).then(|| "true".to_owned())
    };
    let glyph = {
        let row = row.clone();
        move || row().and_then(|row| row.glyph).unwrap_or("").to_owned()
    };
    let tint = {
        let row = row.clone();
        move || {
            row()
                .and_then(|row| row.tint)
                .map(|tint| format!("var(--{tint})"))
        }
    };
    let detail = {
        let row = row.clone();
        move || row().map_or_else(String::new, |row| row.detail)
    };

    view! {
        row(
            class = "picker__row",
            attr:data-selected = selected,
            a11y:role = Role::ListBoxOption,
            on:pointer_down = {
                let picker = picker.clone();
                move |_| picker.go_to(index)
            },
            on:double_click = {
                let picker = picker.clone();
                move |_| {
                    picker.go_to(index);
                    picker.activate();
                }
            }
        ) {
            label(class = "glyph", style:color = tint) {{glyph}}
            row(class = "picker__label nowrap") {
                {move || {
                    let row = row().unwrap_or_else(|| Row::plain("", crate::picker::Target::Nothing));
                    highlighted(&row.label, &row.matched)
                }}
            }
            label(class = "picker__detail nowrap") {{detail}}
        }
    }
}

/// A label with the characters the query landed on picked out.
///
/// One span per run rather than one per character: a path of forty characters that matched six
/// makes seven spans this way and forty the other, and a picker redraws this on every keystroke.
fn highlighted(label: &str, matched: &[u32]) -> Vec<zgui::view::AnyView> {
    use crate::ui::Erase;

    if matched.is_empty() {
        return vec![view! { label(class = "nowrap") {{label.to_owned()}} }.any()];
    }

    let mut spans = Vec::new();
    let mut run = String::new();
    let mut hot = false;
    for (index, character) in label.chars().enumerate() {
        let lit = matched.binary_search(&(index as u32)).is_ok();
        if lit != hot && !run.is_empty() {
            spans.push(span(std::mem::take(&mut run), hot));
        }
        hot = lit;
        run.push(character);
    }
    if !run.is_empty() {
        spans.push(span(run, hot));
    }
    spans
}

/// One run of characters, lit or not.
fn span(text: String, lit: bool) -> zgui::view::AnyView {
    use crate::ui::Erase;
    let class = if lit { "picker__hit" } else { "" };
    view! { label(class = class) {{text}} }.any()
}

/// What the caret is on, down the right.
///
/// Takes whether it is shown rather than being left out by the caller, so that the editor — and
/// the syntax worker behind it — is built only for the sources that have something to preview.
#[component]
fn Preview(
    /// Whether this source previews anything.
    shown: bool,
) -> impl IntoView {
    use crate::ui::Erase;

    if !shown {
        return ().any();
    }
    let picker = use_picker();
    let language: RwSignal<Option<String>, LocalStorage> = RwSignal::new_local(None);
    let handle: RwSignal<Option<zgui_editor::EditorHandle>, LocalStorage> =
        RwSignal::new_local(None);
    let empty = RwSignal::new_local(true);

    // What the caret is on, once it has stopped moving. Held so that a newer selection cancels the
    // read the last one started.
    let waiting: std::rc::Rc<std::cell::RefCell<Option<zgui::view::time::TimeoutHandle>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));

    // Which file is on screen, so that moving the caret between two hits in the same file
    // scrolls rather than re-reading it — which is what made the preview flash on every
    // keystroke of a search.
    let showing: RwSignal<Option<std::path::PathBuf>, LocalStorage> = RwSignal::new_local(None);

    let following = {
        let picker = picker.clone();
        let waiting = std::rc::Rc::clone(&waiting);
        zgui::reactive::RenderEffect::new(move |_| {
            // Read first, so this runs again when either changes.
            let (_, at) = (picker.len(), picker.at());
            let _ = at;
            let Some(preview) = picker.selected().and_then(|row| row.preview()) else {
                if let Some(handle) = handle.get_untracked() {
                    handle.set_text("");
                    handle.clear_decorations(MATCH_LAYER);
                }
                showing.set(None);
                empty.set(true);
                return;
            };
            let Some(timers) = zgui::view::time::Timers::current() else {
                return;
            };

            *waiting.borrow_mut() = Some(timers.set_timeout(PREVIEW_DEBOUNCE, move || {
                let Some(handle) = handle.get_untracked() else {
                    return;
                };
                // Already showing this file: only the place in it has moved.
                if showing.get_untracked().as_deref() == Some(preview.path.as_path()) {
                    show_place(&handle, &preview);
                    return;
                }

                let reading = preview.path.clone();
                crate::task::detached(async move {
                    let text = zgui::task::blocking(move || head_of(&reading)).await;
                    handle.set_text(&text.unwrap_or_default());
                    showing.set(Some(preview.path.clone()));
                    empty.set(false);

                    let named = zdt_core::language::of(&preview.path)
                        .language
                        .map(str::to_owned);
                    if language.get_untracked() != named {
                        language.set(named.clone());
                        handle.set_language(named.as_deref());
                    }
                    show_place(&handle, &preview);
                });
            }));
        })
    };
    on_cleanup_local(move || drop(following));

    view! {
        box(
            class = "picker__preview",
            attr:data-empty = move || empty.get().then(|| "true".to_owned())
        ) {
            Editor(
                class = "picker__editor",
                text = "",
                // Never focused, so never typed into: the prompt holds the keyboard for as long
                // as the picker is open, which is what makes this read-only without a flag.
                autofocus = false,
                config = preview_config(),
                on_ready = Box::new(move |ready: zgui_editor::EditorHandle| {
                    handle.set(Some(ready));
                }) as Box<dyn Fn(zgui_editor::EditorHandle)>,
            )
        }
    }
    .any()
}

/// How the preview behaves: no caret of its own, no animation, and a gutter for reading against.
fn preview_config() -> zgui_editor::EditorConfig {
    zgui_editor::EditorConfig {
        gutter: zgui_editor::GutterMode::Absolute,
        blink: false,
        smooth_scroll: false,
        ..zgui_editor::EditorConfig::default()
    }
}

/// The head of a file, as text.
///
/// Blocking. Bytes that are not text are replaced rather than refused, because a preview that
/// declines to show a file is less use than one that shows it imperfectly.
fn head_of(path: &std::path::Path) -> Option<String> {
    use std::io::Read;

    let file = std::fs::File::open(path).ok()?;
    let mut head = Vec::new();
    file.take(PREVIEW_HEAD).read_to_end(&mut head).ok()?;
    // A file that is not text at all previews as nothing rather than as a screen of replacement
    // characters.
    if head.contains(&0) {
        return None;
    }
    Some(String::from_utf8_lossy(&head).into_owned())
}
