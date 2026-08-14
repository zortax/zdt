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
                .is_some_and(|state| state.current == buffer)
        }
    };
    let dirty = {
        let entry = entry.clone();
        move || entry.is_dirty()
    };

    let window = use_window();

    view! {
        control(
            class = "tab",
            tabindex = Focus::Programmatic,
            attr:data-current = move || current().then(|| "true".to_owned()),
            a11y:label = name.clone(),
            on:pointer_down = window.no_drag_handler(),
            on:click = {
                let workspace = workspace.clone();
                move |_| workspace.show(buffer)
            }
        ) {
            label(class = "glyph", style:color = move || Some(tint.clone())) {{glyph}}
            label(class = "tab__name") {{name}}
            // Always in the row, so a buffer becoming dirty does not shift the text beside it.
            label(class = "tab__mark") {{move || if dirty() { "\u{25cf}" } else { "" }}}
        }
    }
    .any()
}
