//! One window, and the editors it keeps ready.
//!
//! A window shows one buffer, and *holds* every buffer it has recently shown. Each is a mounted
//! editor, and all but the current one are taken out of the flow. That is what makes `]b` and
//! `<Leader>ff` onto something already visited instant. The scroll position, the selections and
//! the parsed tree are all still there, because nothing was unmounted.
//!
//! One editor whose document is swapped cannot work. A document is what an editor is built around,
//! and swapping it means unmounting anyway. That loses the view state and re-parses the file on
//! every buffer switch.

use std::cell::RefCell;
use std::rc::Rc;

use zgui::prelude::*;
use zgui::reactive::RenderEffect;
use zgui::{component, view};
use zgui_editor::{EditorConfig, EditorHandle, EditorProps, GutterMode};

use crate::leap::view::LeapLabelsProps;
use crate::rich::{MarkdownPreviewProps, ViewPillProps};
use crate::settings::use_settings;
use crate::settings::view::ConfigPanelProps;
use crate::terminals::view::EmulatorProps;
use crate::vim::use_vim;
use crate::workspace::{BufferId, BufferKind, WindowId, use_workspace};
use zdt_gitui::GitPanelProps;
use zdt_icons::IconProps;
use zdt_view::Erase;

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
    // change on two existing views, and never a mount and an unmount.
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

    // A window showing nothing is a real state, not a mistake to be papered over with an empty
    // scratch buffer that nobody asked for and that then sits on the buffer line.
    let empty = {
        let workspace = workspace.clone();
        move || {
            workspace
                .window(window)
                .is_none_or(|state| state.current.is_none())
        }
    };

    view! {
        box(
            class = "pane",
            attr:data-focused = move || focused().then(|| "true".to_owned()),
            // Whether the caret's own line is tinted at all. The colour belongs to the sheet;
            // which window draws one is told to the editor, because a colour that changes because
            // an attribute changed on an ancestor reaches it on no frame at all.
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
            {move || {
                use zdt_view::Erase;
                if empty() { view! { Nothing() }.any() } else { ().any() }
            }}
        }
    }
}

/// What a window with no buffer in it shows.
///
/// Two lines saying what is true and how to change it. No hint sheet, and no dashboard.
#[component]
fn Nothing() -> impl IntoView {
    view! {
        column(class = "pane__empty") {
            Icon(icon = zdt_icons::FILE, class = "pane__empty-icon")
            label(class = "pane__empty-title") {"No buffer open"}
            label(class = "pane__empty-hint") {"<Space>ff to find a file   <Space>n for a new one"}
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
                .is_some_and(|state| state.current == Some(buffer))
        }
    };

    match &entry.kind {
        BufferKind::Text { document } => {
            // Whether this buffer has a rich form at all. Decided once: the file type is fixed
            // when the buffer is made.
            let rich_kind = crate::rich::RichKind::of(&entry);
            let settings = use_settings();
            let config = settings.with(|held| EditorConfig {
                gutter: gutter_of(held.editor.line_numbers),
                cursor_style: zgui_editor::CursorStyle::Block,
                scrolloff: held.editor.scrolloff,
                smooth_scroll: held.editor.smooth_scroll,
                glide_threshold_lines: held.editor.smooth_scroll_min_lines,
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

            // The language is set through the handle, because the prop takes a name and this
            // has an answer that may be "none".
            // Kept so that the cleanup below can say *which* editor is going away. A pane rebuilt
            // in place registers its new editor before the old one is cleaned up, and a cleanup
            // that only named the window and the buffer would take the new registration with it.
            let mine: Rc<RefCell<Option<EditorHandle>>> = Rc::new(RefCell::new(None));
            let on_ready = {
                let workspace = workspace.clone();
                let language = entry.language();
                let mine = Rc::clone(&mine);
                let ready = crate::session::use_session();
                Box::new(move |handle: EditorHandle| {
                    handle.set_language(language);
                    *mine.borrow_mut() = Some(handle.clone());
                    // Where it starts out, so a buffer nobody has touched still has a recorded
                    // place. Everything after this comes from the editor's own events.
                    if let Some(writer) = ready.as_ref().and_then(crate::session::Session::writer) {
                        writer.remember_view(window, buffer, &handle);
                    }
                    workspace.register_handle(window, buffer, handle);
                }) as Box<dyn Fn(EditorHandle)>
            };

            // The revision the buffer line's dirty mark is decided by. Written from here because
            // this is the only thing that hears the editor.
            let on_event = {
                let entry = entry.clone();
                let language = zgui::reactive::use_local_context::<crate::language::Language>();
                let completion =
                    zgui::reactive::use_local_context::<crate::completion::Completion>();
                let workspace = workspace.clone();
                let mine = Rc::clone(&mine);
                // Taken here, where there certainly is one: this closure runs from the editor.
                let session = crate::session::use_session();
                Box::new(move |event: zgui_editor::EditorEvent| {
                    match event {
                        zgui_editor::EditorEvent::Edited { ref kind, .. } => {
                            // Whether it is dirty is a question about the *text*, not the
                            // revision: undoing back to what is on disk gives a new revision, not
                            // the old one.
                            entry.refresh_dirty();
                            // And the session hears about it after a longer pause, so typing a
                            // sentence is one write.
                            if let Some(session) = session.as_ref() {
                                session.touched_text(buffer);
                            }
                            // The servers hear about it after a pause, so typing a word is one
                            // notification.
                            if let Some(language) = language.as_ref() {
                                language.changed(buffer);
                            }
                            // Typing offers suggestions. Anything else that changes the text
                            // puts them away: a paste, an undo, a formatter. None of those is
                            // somebody in the middle of a word.
                            if let Some(completion) = completion.as_ref() {
                                match kind {
                                    zgui_editor::EditKind::Typing
                                    | zgui_editor::EditKind::Deletion => {
                                        let handle = mine.borrow().clone();
                                        completion.typed(&workspace, handle.as_ref());
                                    }
                                    _ => completion.close(),
                                }
                            }
                        }
                        // The caret moving, the view moving, the keyboard leaving: all three mean
                        // the popup is about somewhere the caret no longer is.
                        zgui_editor::EditorEvent::SelectionMoved
                        | zgui_editor::EditorEvent::Scrolled => {
                            if let Some(completion) = completion.as_ref() {
                                completion.close();
                            }
                            // And where this editor is looking, kept for the session. Only on
                            // the two events that mean it *moved*: the keyboard leaving is also
                            // what a view being taken apart reports, and an editor whose scope
                            // is being disposed of answers a signal read by panicking.
                            if let Some(writer) =
                                session.as_ref().and_then(crate::session::Session::writer)
                                && let Some(handle) = mine.borrow().as_ref()
                            {
                                writer.remember_view(window, buffer, handle);
                            }
                        }
                        // The keyboard leaving means the popup is about somewhere the caret no
                        // longer is, and nothing more: a blur moves no view.
                        zgui_editor::EditorEvent::Blurred => {
                            if let Some(completion) = completion.as_ref() {
                                completion.close();
                            }
                        }
                        _ => {}
                    }
                }) as Box<dyn Fn(zgui_editor::EditorEvent)>
            };

            {
                let workspace = workspace.clone();
                let mine = Rc::clone(&mine);
                on_cleanup_local(move || {
                    if let Some(handle) = mine.borrow().as_ref() {
                        // Nothing here asks the editor anything. This runs while the scope is
                        // being disposed of, where the editor's own signals are already gone,
                        // and a panic in a destructor aborts rather than unwinds. Where it was
                        // looking was recorded as it moved; see `on_event` above.
                        workspace.forget_handle(window, buffer, handle);
                    }
                });
            }

            // Which view is being worked in, which the caret's own line band follows. Followed and
            // never set once, because both the current buffer and the focused window change under
            // it.
            //
            // Told to the editor, and never left to a class: the editor reads its colours off the
            // computed style during layout, and a colour that changes because an attribute changed
            // on an ancestor reaches it on no frame at all.
            //
            // Nothing here asks for the keyboard. The projector is the one thing that gives a node
            // focus, and it already knows an editor is how a text buffer takes it.
            {
                let workspace = workspace.clone();
                let active = RenderEffect::new(move |_| {
                    // Read first: an editor mounting is what has to wake this, and it is the one
                    // thing that changes without the window or the focus changing.
                    let _ = workspace.mounted_revision();
                    let current = workspace
                        .window(window)
                        .is_some_and(|state| state.current == Some(buffer));
                    if let Some(handle) = workspace.handle_for(window, buffer) {
                        handle.set_active(current && workspace.focused() == window);
                    }
                });
                on_cleanup_local(move || drop(active));
            }

            // What the servers say, painted into the editor's own decoration layer.
            if let Some(language) = zgui::reactive::use_local_context::<crate::language::Language>()
            {
                let following =
                    crate::language::diagnostics::follow(&workspace, &language, window, buffer);
                on_cleanup_local(move || drop(following));
            }

            // And what git says, in a layer of its own beside them.
            if let Some(git) = zgui::reactive::use_local_context::<crate::git::Git>() {
                let following =
                    crate::language::diagnostics::follow_git(&workspace, &git, window, buffer);
                on_cleanup_local(move || drop(following));
            }

            // The settings the editor can be told about after it is mounted. The rest are CSS
            // and reach it through the cascade: the fonts, the tab width.
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

            // Every key reaches the modal layer before the editor does, which is the whole seam
            // a vim mode needs. A key it declines falls through to the editor's own handling. That
            // is what makes typing in insert mode the editor's business, with its auto-indent and
            // its undo grouping.
            let font_step = {
                let workspace = workspace.clone();
                move || {
                    let step = workspace.font_step(window);
                    (step != 0).then(|| step.to_string())
                }
            };

            let vim = use_vim();
            let on_key: zgui_editor::KeyFilter = Box::new(
                move |event: &zgui::vocab::KeyEvent,
                      modifiers: zgui::vocab::Modifiers,
                      handle: &EditorHandle| {
                    match crate::keys::chord_of(event, modifiers) {
                        Some(chord) => vim.key(chord, crate::vim::Surface::Editor(handle)),
                        // A modifier on its own, or a key the keymap has no word for.
                        None => false,
                    }
                },
            );

            // While the split shows the rich form, the editor is out of the flow the same way a
            // buffer not being shown is: hidden and warm, with its handle still filed.
            let editor_display = {
                let workspace = workspace.clone();
                move || workspace.is_rich(window, buffer).then(|| "none".to_owned())
            };

            // The rich view mounts on the first toggle and stays for the pane's life, so its
            // parse and its reading position survive a toggle back. The latch moves once, which
            // is what keeps the hole below from rebuilding the view on every toggle.
            let preview = if rich_kind.is_some() {
                let latched: zgui::reactive::RwSignal<bool, zgui::reactive::LocalStorage> =
                    zgui::reactive::RwSignal::new_local(
                        workspace.is_rich_untracked(window, buffer),
                    );
                {
                    let workspace = workspace.clone();
                    let arming = RenderEffect::new(move |_| {
                        if workspace.is_rich(window, buffer) && !latched.get_untracked() {
                            latched.set(true);
                        }
                    });
                    on_cleanup_local(move || drop(arming));
                }
                let workspace = workspace.clone();
                view! {
                    {(move || {
                        if !latched.get() {
                            return ().any();
                        }
                        let workspace = workspace.clone();
                        view! {
                            box(
                                class = "pane__preview",
                                style:display = move || {
                                    (!workspace.is_rich(window, buffer))
                                        .then(|| "none".to_owned())
                                }
                            ) {
                                MarkdownPreview(window = window, buffer = buffer)
                            }
                        }
                        .any()
                    })}
                }
                .any()
            } else {
                ().any()
            };
            let pill = if rich_kind.is_some() {
                view! { ViewPill(window = window, buffer = buffer) }.any()
            } else {
                ().any()
            };

            view! {
                box(
                    class = "pane__buffer",
                    style:display = move || (!current()).then(|| "none".to_owned())
                ) {
                    // The braced children first: a braced child after a parenthesised element
                    // would read as that element's children block.
                    {preview}
                    {pill}
                    LeapLabels(window = window, buffer = buffer)
                    Editor(
                        class = "pane__editor",
                        // This window's own size, so `<C-+>` in a split grows that split alone.
                        // A custom property, and never `font-size` itself. The editor reads its
                        // metrics off the computed style, and the sheet decides what to do with
                        // the number.
                        style:--zdt-pane-font-step = font_step,
                        style:display = editor_display,
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
        // The emulator hides itself when its window is showing something else, for the same
        // reason an editor does: a terminal taken out of the tree is a program shut down.
        BufferKind::Terminal { .. } => view! {
            box(class = "pane__buffer") {
                Emulator(buffer = buffer, window = Some(window))
            }
        }
        .any(),
        // A panel is a page: no editor, no decorations, nothing to save. It is a buffer so that
        // the buffer line, the splits and every key that walks between tabs work on it without
        // being told about it.
        BufferKind::Settings => view! {
            box(class = "pane__buffer pane__panel") { ConfigPanel() }
        }
        .any(),
        BufferKind::Git => {
            // Where the keyboard lands for this tab. A panel is no editor, so nothing else in the
            // workspace can answer for it.
            let panel = zgui::prelude::NodeRef::new();
            crate::focus::claim::sink(
                crate::focus::Spot::Buffer(window, buffer),
                crate::focus::Sink::Node(panel),
            );
            view! {
                box(class = "pane__buffer pane__panel") { GitPanel(element_ref = panel) }
            }
            .any()
        }
    }
}
