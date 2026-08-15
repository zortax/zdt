//! One window, and the editors it keeps ready.
//!
//! A window shows one buffer, but it *holds* every buffer it has recently shown: each is a mounted
//! editor, and all but the current one are taken out of the flow. That is what makes `]b` and
//! `<Leader>ff` onto something already visited instant — the scroll position, the selections and
//! the parsed tree are all still there, because nothing was unmounted.
//!
//! The alternative, one editor whose document is swapped, cannot work: a document is what an
//! editor is built around, and swapping it would mean unmounting anyway, losing the view state and
//! re-parsing the file on every buffer switch.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use zgui::prelude::*;
use zgui::reactive::RenderEffect;
use zgui::view::time::{TimeoutHandle, Timers};
use zgui::{component, view};
use zgui_editor::{EditorConfig, EditorHandle, EditorProps, GutterMode};

use crate::settings::use_settings;
use crate::ui::Erase;
use crate::vim::use_vim;
use crate::workspace::{BufferId, BufferKind, WindowId, use_workspace};

/// One window.
// The list macro takes a closure by construction, so the one it is handed here is not redundant.
#[allow(clippy::redundant_closure)]
#[component]
pub fn Pane(
    /// Which window this is.
    window: WindowId,
) -> impl IntoView {
    let workspace = use_workspace();

    // Which buffers this window has an editor for. Keyed by buffer, so a switch is a class
    // change on two existing views rather than a mount and an unmount.
    let mounted = {
        let workspace = workspace.clone();
        move || {
            workspace
                .window(window)
                .map(|state| state.mounted)
                .unwrap_or_default()
        }
    };

    let focused = {
        let workspace = workspace.clone();
        move || workspace.focused() == window
    };

    view! {
        box(
            class = "pane",
            attr:data-focused = move || focused().then(|| "true".to_owned()),
            // The caret's own line is tinted unless somebody said not to. A class rather than a
            // command, because it is a colour and colours are the sheet's.
            attr:data-cursorline = {
                let settings = use_settings();
                move || (!settings.with(|config| config.editor.cursorline)).then(|| "off".to_owned())
            },
            on:pointer_down = {
                let workspace = workspace.clone();
                move |_| workspace.focus_window(window)
            }
        ) {
            for buffer in move || mounted(), key = |buffer: &BufferId| *buffer {
                BufferView(window = window, buffer = buffer)
            }
        }
    }
}

/// How the gutter numbers its lines, as the editor says it.
fn gutter_of(numbers: zdt_core::config::LineNumbers) -> GutterMode {
    match numbers {
        zdt_core::config::LineNumbers::Absolute => GutterMode::Absolute,
        zdt_core::config::LineNumbers::Relative => GutterMode::Relative,
        zdt_core::config::LineNumbers::None => GutterMode::None,
    }
}

/// One buffer, as one window shows it.
#[component]
fn BufferView(
    /// Which window it is in.
    window: WindowId,
    /// Which buffer it shows.
    buffer: BufferId,
) -> impl IntoView {
    let workspace = use_workspace();
    let Some(entry) = workspace.buffer_untracked(buffer) else {
        // The buffer closed between the window listing it and this mounting. Nothing to show.
        return view! { box() }.any();
    };

    let current = {
        let workspace = workspace.clone();
        move || {
            workspace
                .window(window)
                .is_some_and(|state| state.current == buffer)
        }
    };

    match &entry.kind {
        BufferKind::Text { document } => {
            let settings = use_settings();
            let config = settings.with(|held| EditorConfig {
                gutter: gutter_of(held.editor.line_numbers),
                cursor_style: zgui_editor::CursorStyle::Block,
                scrolloff: held.editor.scrolloff,
                smooth_scroll: held.editor.smooth_scroll,
                edit: zgui_editor::EditOptions {
                    indent: if held.editor.expand_tab {
                        " ".repeat(held.editor.tab_size.clamp(1, 16) as usize)
                    } else {
                        "\t".to_owned()
                    },
                    ..zgui_editor::EditOptions::default()
                },
                ..EditorConfig::default()
            });

            // The language is set through the handle rather than through the prop, because the
            // prop takes a name and this has an answer that may be "none".
            let on_ready = {
                let workspace = workspace.clone();
                let language = entry.language();
                Box::new(move |handle: EditorHandle| {
                    handle.set_language(language);
                    workspace.register_handle(window, buffer, handle);
                }) as Box<dyn Fn(EditorHandle)>
            };

            // The revision the buffer line's dirty mark is decided by. Written from here because
            // this is the only thing that hears the editor.
            let on_event = {
                let revision = entry.revision;
                Box::new(move |event: zgui_editor::EditorEvent| {
                    if let zgui_editor::EditorEvent::Edited { revision: at, .. } = event {
                        revision.set(at);
                    }
                }) as Box<dyn Fn(zgui_editor::EditorEvent)>
            };

            {
                let workspace = workspace.clone();
                on_cleanup_local(move || workspace.forget_handle(window, buffer));
            }

            // The editor with the keyboard is the current buffer of the focused window. Followed
            // rather than set once, because both of those change under it — a `]b` or a `<C-w>w`
            // has to move the keyboard as well as the view.
            //
            // The claim is made from a timer rather than here, because the first run of this
            // effect happens while the editor is still being built and a node that is not mounted
            // cannot take focus. The handle is held for the component's life: dropping a timer
            // cancels it.
            {
                let workspace = workspace.clone();
                let timers = Timers::current();
                let claim: Rc<RefCell<Option<TimeoutHandle>>> = Rc::new(RefCell::new(None));
                let held = Rc::clone(&claim);
                let focus = RenderEffect::new(move |_| {
                    let current = workspace
                        .window(window)
                        .is_some_and(|state| state.current == buffer);
                    if !current || workspace.focused() != window {
                        return;
                    }
                    let Some(timers) = timers.as_ref() else {
                        return;
                    };
                    let workspace = workspace.clone();
                    *held.borrow_mut() = Some(timers.set_timeout(Duration::ZERO, move || {
                        if let Some(handle) = workspace.handle_for(window, buffer) {
                            handle.focus();
                        }
                    }));
                });
                on_cleanup_local(move || {
                    drop(focus);
                    drop(claim);
                });
            }

            // The settings that the editor can be told about after it is mounted. The rest — the
            // fonts, the tab width — are CSS and reach it through the cascade.
            {
                let settings = settings.clone();
                let workspace = workspace.clone();
                let vim = use_vim();
                let following = RenderEffect::new(move |previous: Option<()>| {
                    let _ = settings.with(|config| config.editor.line_numbers);
                    // The first run is the config the editor was just built with.
                    if previous.is_some()
                        && let Some(handle) = workspace.handle_for(window, buffer)
                    {
                        vim.refresh(&handle);
                    }
                });
                on_cleanup_local(move || drop(following));
            }

            // Every key reaches the modal layer before the editor does, which is the whole seam a
            // vim mode needs. A key it declines falls through to the editor's own handling —
            // which is what makes typing in insert mode the editor's business, with its
            // auto-indent and its undo grouping.
            let vim = use_vim();
            let on_key: zgui_editor::KeyFilter = Box::new(
                move |event: &zgui::vocab::KeyEvent,
                      modifiers: zgui::vocab::Modifiers,
                      handle: &EditorHandle| {
                    match crate::keys::chord_of(event, modifiers) {
                        Some(chord) => vim.key(chord, handle),
                        // A modifier on its own, or a key the keymap has no word for.
                        None => false,
                    }
                },
            );

            view! {
                box(
                    class = "pane__buffer",
                    style:display = move || (!current()).then(|| "none".to_owned())
                ) {
                    Editor(
                        class = "pane__editor",
                        document = document.clone(),
                        config = config,
                        autofocus = false,
                        on_ready = on_ready,
                        on_event = on_event,
                        on_key = on_key,
                    )
                }
            }
            .any()
        }
        BufferKind::Terminal { .. } => view! {
            box(
                class = "pane__buffer pane__buffer--pending",
                style:display = move || (!current()).then(|| "none".to_owned())
            ) {
                label(class = "muted") {"terminal buffers arrive with the terminal"}
            }
        }
        .any(),
    }
}
