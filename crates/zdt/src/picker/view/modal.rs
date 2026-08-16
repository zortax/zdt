//! The modal, and the box inside it.

use crate::picker::use_picker;
use crate::picker::view::{MatchesProps, PreviewProps};
use std::time::Duration;
use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui::prelude::*;
use zgui_ui_primitives::prelude::*;

/// The modal.
#[component]
pub fn Picker() -> impl IntoView {
    let picker = use_picker();
    let surface = NodeRef::new();

    // What it was opened as, kept for as long as it is on the screen. That is a little longer
    // than it is *open*. The source is cleared the moment a picker closes, and a modal that read
    // it directly would spend its whole exit animation blank.
    let showing: RwSignal<Option<crate::picker::Source>, LocalStorage> = RwSignal::new_local(None);
    let follow = {
        let picker = picker.clone();
        zgui::reactive::RenderEffect::new(move |_| {
            if let Some(source) = picker.source() {
                showing.set(Some(source));
            }
        })
    };
    on_cleanup_local(move || drop(follow));

    let present = {
        let picker = picker.clone();
        Signal::derive_local(move || picker.source().is_some())
    };

    view! {
        Presence(present = present, surface = surface) {
            {move || {
                use zdt_view::Erase;

                match showing.get() {
                    Some(source) => view! {
                        Open(
                            title = source.title(),
                            previews = source.previews(),
                            surface = surface,
                        )
                    }
                    .any(),
                    None => ().any(),
                }
            }}
        }
    }
}

/// One picker, for as long as it is open.
///
/// Built fresh per opening, and never kept and hidden. The preview editor holds a document and a
/// syntax worker that a closed picker has no use for.
#[component]
pub(crate) fn Open(
    /// What the picker calls itself.
    title: &'static str,
    /// Whether to show what the caret is on beside the list.
    previews: bool,
    /// The modal's own element, whose exit animation says when it may be taken away.
    surface: NodeRef,
) -> impl IntoView {
    let picker = use_picker();
    let leaving = use_presence();
    let field = NodeRef::new();
    let query = RwSignal::new_local(picker.query());

    // From a timer, because a node that is not mounted cannot take focus.
    let claim = zgui::view::time::Timers::current()
        .map(|timers| timers.set_timeout(Duration::ZERO, move || field.focus()));
    on_cleanup_local(move || drop(claim));

    // What is typed reaches the picker through here, and not through the field's own binding, so
    // the search starts on the keystroke and not on the frame after it.
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
        box(
            class = "picker__scrim",
            attr:data-state = move || zdt_view::leaving_state(leaving),
            on:pointer_down = {
                let picker = picker.clone();
                move |_| picker.close()
            }
        ) {}

        column(
            class = "picker",
            node_ref = surface,
            attr:data-state = move || zdt_view::leaving_state(leaving),
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
