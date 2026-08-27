//! The floating raw/rich toggle in a split's corner.
//!
//! Two pills, one lit, the same shape as the agent sidebar's face toggle. Pressing the one
//! already lit changes nothing. The press bubbles on to the pane, which focuses the split, and
//! the projector then puts the keyboard where the new form takes it.

use zdt_icons::{self as icons, IconProps};
use zgui::prelude::*;
use zgui::{component, view};

use crate::vim::use_vim;
use crate::workspace::{BufferId, WindowId, use_workspace};

#[component]
pub fn ViewPill(
    /// Which window it is in.
    window: WindowId,
    /// Which buffer it switches.
    buffer: BufferId,
) -> impl IntoView {
    let workspace = use_workspace();
    let vim = use_vim();

    let showing_source = {
        let workspace = workspace.clone();
        move || (!workspace.is_rich(window, buffer)).then(|| "true".to_owned())
    };
    let showing_rich = {
        let workspace = workspace.clone();
        move || workspace.is_rich(window, buffer).then(|| "true".to_owned())
    };
    let to_source = {
        let workspace = workspace.clone();
        move |_: &mut EventCx<'_, events::PointerDown>| {
            if workspace.is_rich_untracked(window, buffer) {
                workspace.toggle_rich(window, buffer);
            }
        }
    };
    let to_rich = {
        let workspace = workspace.clone();
        let vim = vim.clone();
        move |_: &mut EventCx<'_, events::PointerDown>| {
            if !workspace.is_rich_untracked(window, buffer) {
                // The page has no caret, so whatever the engine was in the middle of ends here.
                vim.reset();
                workspace.toggle_rich(window, buffer);
            }
        }
    };

    view! {
        row(class = "viewpill", a11y:role = Role::TabList) {
            control(
                class = "viewpill__face",
                tabindex = Focus::Programmatic,
                a11y:label = "Source view",
                attr:data-on = showing_source,
                on:pointer_down = to_source
            ) {
                Icon(icon = icons::CODE_XML, class = "icon--xs")
                label(class = "nowrap") {"Raw"}
            }
            control(
                class = "viewpill__face",
                tabindex = Focus::Programmatic,
                a11y:label = "Rich view",
                attr:data-on = showing_rich,
                on:pointer_down = to_rich
            ) {
                Icon(icon = icons::BOOK_OPEN, class = "icon--xs")
                label(class = "nowrap") {"Rich"}
            }
        }
    }
}
