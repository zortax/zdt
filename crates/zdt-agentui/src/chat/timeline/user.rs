//! One message a person sent, as a row.

use zdt_agent::thread::TimelineItem;
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::{component, view};

use crate::use_agent;

/// One user message: the words in a real editable element, so a pointer can select in it.
///
/// Read-only in practice rather than in state, because the framework only selects in editable
/// elements. Motion and copy keys pass through, `y` copies the selection like a yank, and every
/// key that would change the text is eaten before the editing default runs.
#[component]
pub(super) fn UserRow(
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
