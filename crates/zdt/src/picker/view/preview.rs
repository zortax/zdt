//! The preview beside the matches.

use super::*;
use crate::picker::use_picker;
use zgui::prelude::*;
use zgui::{component, view};
use zgui_editor::EditorProps;

/// What the caret is on, down the right.
///
/// Takes whether it is shown, so the caller never leaves it out. The editor, and the syntax
/// worker behind it, is then built only for the sources that have something to preview.
#[component]
pub(crate) fn Preview(
    /// Whether this source previews anything.
    shown: bool,
) -> impl IntoView {
    use zdt_view::Erase;

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

    // Which file is on screen, so moving the caret between two hits in the same file scrolls.
    // Re-reading it is what made the preview flash on every keystroke of a search.
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
                zdt_view::detached(async move {
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
/// Blocking. Bytes that are not text are replaced. A preview that declines to show a file is less
/// use than one that shows it imperfectly.
fn head_of(path: &std::path::Path) -> Option<String> {
    use std::io::Read;

    let file = std::fs::File::open(path).ok()?;
    let mut head = Vec::new();
    file.take(PREVIEW_HEAD).read_to_end(&mut head).ok()?;
    // A file that is not text at all previews as nothing. A screen of replacement characters
    // helps nobody.
    if head.contains(&0) {
        return None;
    }
    Some(String::from_utf8_lossy(&head).into_owned())
}
