//! The strip across the top: the menu, what is open, and the window buttons.
//!
//! One row does the work of three: it is the buffer line, the header, and the handle the desktop
//! moves the window by. A press anywhere in it that is not on a control drags the window; a second
//! press inside the double-press interval maximises it.

use zgui::prelude::*;
use zgui::{component, view};

use crate::icons::{self, IconProps};
use crate::ui::Erase;
use crate::ui::frame::WindowControlsProps;
use crate::workspace::{BufferId, use_workspace};

/// The whole strip.
// The list macro takes a closure by construction, so the one it is handed here is not redundant.
#[allow(clippy::redundant_closure)]
#[component]
pub fn Chrome() -> impl IntoView {
    let window = use_window();
    let workspace = use_workspace();

    let branch = {
        let workspace = workspace.clone();
        // Read once: a branch changes when a person runs git, and the file watcher will say so
        // once there is one. Reading it every frame would be a file read every frame.
        workspace.project().git_branch()
    };

    let order = {
        let workspace = workspace.clone();
        move || workspace.order()
    };

    view! {
        row(class = "chrome", on:pointer_down = window.move_drag_handler()) {
            control(
                class = "chrome__button",
                tabindex = Focus::Programmatic,
                a11y:label = "Menu",
                on:pointer_down = window.no_drag_handler()
            ) {
                Icon(icon = icons::MENU, class = "icon--sm")
            }

            row(class = "bufferline") {
                for buffer in move || order(), key = |buffer: &BufferId| *buffer {
                    BufferTab(buffer = buffer)
                }
            }

            box(class = "fill") {}

            {branch.map(|branch| view! {
                row(class = "chrome__branch") {
                    Icon(icon = icons::GIT_BRANCH, class = "icon--sm")
                    label {{branch}}
                }
            })}

            WindowControls()
        }
    }
}

/// Closes a buffer, keeping one that has unsaved changes.
///
/// The same rule the `<Leader>c` key follows: a mouse is not a reason to lose work, and the way
/// to close anyway is the key that says so.
fn close(workspace: &crate::workspace::Workspace, buffer: BufferId) {
    let dirty = workspace
        .buffer_untracked(buffer)
        .is_some_and(|entry| entry.is_dirty());
    if dirty {
        workspace.show(buffer);
        workspace.complain("unsaved changes; <Leader>C closes anyway");
    } else {
        workspace.close_buffer(buffer);
    }
}

/// One buffer, on the buffer line.
#[component]
fn BufferTab(
    /// Which buffer it stands for.
    buffer: BufferId,
) -> zgui::view::AnyView {
    let workspace = use_workspace();
    let Some(entry) = workspace.buffer_untracked(buffer) else {
        // The buffer closed between the line listing it and this being built.
        return view! { box() }.any();
    };

    let name = entry.name();
    let glyph = entry.file_type.glyph;
    let tint = format!("var(--{})", entry.file_type.tint);

    let current = {
        let workspace = workspace.clone();
        move || {
            workspace
                .window(workspace.focused())
                .is_some_and(|state| state.current == Some(buffer))
        }
    };
    let dirty = {
        let entry = entry.clone();
        move || entry.is_dirty()
    };
    // The key that goes here, while the tabs are labelled. It takes the whole slot: the mark and
    // the close button are not what is being asked about.
    let key = {
        let tabs = crate::tabpick::use_tabpick();
        move || tabs.label_for(buffer).map(|key| key.to_string())
    };

    let window = use_window();

    let drag = window.no_drag_handler();
    let press = {
        let workspace = workspace.clone();
        move |event: &mut EventCx<'_, events::PointerDown>| {
            drag(event);
            match event.button {
                // The middle button closes, as it does on every tab strip.
                Some(PointerButton::Middle) => close(&workspace, buffer),
                // On the press, not the release: a tab that waits for the button to come up feels
                // like it is deciding, and every other tab strip switches on the way down.
                _ => workspace.show(buffer),
            }
        }
    };

    let closing = {
        let workspace = workspace.clone();
        move |event: &mut EventCx<'_, events::PointerDown>| {
            // Before the tab beneath it hears the press: closing a tab is not switching to it.
            event.stop_propagation();
            close(&workspace, buffer);
        }
    };

    view! {
        control(
            class = "tab",
            tabindex = Focus::Programmatic,
            attr:data-current = move || current().then(|| "true".to_owned()),
            attr:data-dirty = move || dirty().then(|| "true".to_owned()),
            attr:data-labelled = {
                let key = key.clone();
                move || key().map(|_| "true".to_owned())
            },
            a11y:label = name.clone(),
            on:pointer_down = press
        ) {
            label(class = "glyph", style:color = move || Some(tint.clone())) {{glyph}}
            label(class = "tab__name") {{name}}
            // One slot, three things: a dot when the buffer is dirty, a cross while the pointer
            // is over the tab, and nothing otherwise — always present, so the name beside it does
            // not shift when a buffer is edited.
            box(class = "tab__slot") {
                label(class = "tab__key") {{move || key().unwrap_or_default()}}
                label(class = "tab__mark") {"\u{25cf}"}
                control(
                    class = "tab__close",
                    tabindex = Focus::Programmatic,
                    a11y:label = "Close",
                    on:pointer_down = closing
                ) {
                    Icon(icon = icons::X, class = "icon--xs")
                }
            }
        }
    }
    .any()
}
