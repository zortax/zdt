//! The file tree.

use super::*;
use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui::prelude::*;

/// What the file tree shows.
#[component]
pub(crate) fn Tree() -> impl IntoView {
    let settings = crate::settings::use_settings();
    let open = bound(
        &settings,
        |config| config.tree.open,
        |config, value| config.tree.open = value,
    );
    let width = number(
        &settings,
        |config| config.tree.width,
        |config, value| config.tree.width = value as u32,
    );
    let hidden = bound(
        &settings,
        |config| config.tree.hidden,
        |config, value| config.tree.hidden = value,
    );
    let ignored = bound(
        &settings,
        |config| config.tree.ignored,
        |config, value| config.tree.ignored = value,
    );
    let follow = bound(
        &settings,
        |config| config.tree.follow,
        |config, value| config.tree.follow = value,
    );

    view! {
        SettingsGroup {
            SettingsGroupLabel {"The panel"}
            SettingsItem(label = "Open it with the window") {
                Switch(class = "config__switch", checked = open, {..use_settings_item_attrs()})
            }
            SettingsItem(label = "How wide it is") {
                Number(value = width, min = 140.0, max = 600.0, step = 10.0, unit = "px")
            }
            SettingsItem(
                label = "Follow the editor",
                description = "Moves the tree's caret onto whatever file is being edited."
            ) {
                Switch(class = "config__switch", checked = follow, {..use_settings_item_attrs()})
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"What it shows"}
            SettingsItem(label = "Files beginning with a dot") {
                Switch(class = "config__switch", checked = hidden, {..use_settings_item_attrs()})
            }
            SettingsItem(label = "Files git ignores") {
                Switch(class = "config__switch", checked = ignored, {..use_settings_item_attrs()})
            }
        }
    }
}
