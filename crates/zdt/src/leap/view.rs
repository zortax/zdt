//! The leap labels.
//!
//! One absolutely-positioned character per place the leap could go, put where the editor says
//! that byte is on screen. Nothing about the editor changes. The labels are ordinary elements over
//! the top of it, which is the whole reason `point_for_byte` was worth adding.
//!
//! Drawn inside the pane, and never over the window. A label is then clipped by the window it
//! belongs to, and two panes never label each other's text.

use zgui::prelude::*;
use zgui::{component, view};

use crate::vim::use_vim;
use crate::workspace::{BufferId, WindowId, use_workspace};

/// The labels over one editor.
#[component]
pub fn LeapLabels(
    /// Which window they belong to.
    window: WindowId,
    /// Which buffer is being labelled.
    buffer: BufferId,
) -> impl IntoView {
    let vim = use_vim();
    let workspace = use_workspace();
    let leaping = vim.leaping();

    // Where each label goes, in the editor's own coordinates. Recomputed whenever the labels
    // change, which is once per leap.
    let placed = {
        let leaping = leaping.clone();
        let workspace = workspace.clone();
        move || {
            let labels = leaping.labels();
            if labels.is_empty() {
                return Vec::new();
            }
            // A leap over the file tree's rows labels rows, and its numbers are row indices. Read
            // as byte offsets they would put labels somewhere in the text.
            if leaping.over() != crate::leap::Over::Text {
                return Vec::new();
            }
            // Only the window with the keyboard: a leap is one caret's, and labels over the other
            // panes would be places its keys cannot reach.
            if workspace.focused() != window {
                return Vec::new();
            }
            let Some(handle) = workspace.handle_for(window, buffer) else {
                return Vec::new();
            };

            labels
                .into_iter()
                .filter_map(|landing| {
                    // A place the editor cannot put on screen is one that has scrolled away since
                    // the labels were worked out. Leaving it out is better than drawing it at the
                    // origin.
                    //
                    // Element-local, because these labels are placed inside a box that fills the
                    // editor: a window coordinate would be out by wherever the pane happens to sit.
                    let rect = handle.local_point_for_byte(landing.at)?;
                    Some((landing.label, rect.x, rect.y, rect.height))
                })
                .collect::<Vec<_>>()
        }
    };

    view! {
        box(class = "leap") {
            {move || {
                placed()
                    .into_iter()
                    .map(|(label, x, y, height)| {
                        view! {
                            label(
                                class = "leap__label",
                                style:left = Some(format!("{x}px")),
                                style:top = Some(format!("{y}px")),
                                style:height = Some(format!("{height}px")),
                            ) {{label.to_string()}}
                        }
                    })
                    .collect::<Vec<_>>()
            }}
        }
    }
}
