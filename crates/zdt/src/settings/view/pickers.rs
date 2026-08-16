//! The pickers.

use super::*;
use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui::prelude::*;

/// How the pickers search.
#[component]
pub(crate) fn Pickers() -> impl IntoView {
    let settings = crate::settings::use_settings();
    let preview = bound(
        &settings,
        |config| config.picker.preview,
        |config, value| config.picker.preview = value,
    );
    let rows = number(
        &settings,
        |config| config.picker.max_results as u32,
        |config, value| config.picker.max_results = value as usize,
    );
    let smart_case = bound(
        &settings,
        |config| config.picker.smart_case,
        |config, value| config.picker.smart_case = value,
    );
    let hidden = bound(
        &settings,
        |config| config.picker.hidden,
        |config, value| config.picker.hidden = value,
    );
    let ignored = bound(
        &settings,
        |config| config.picker.ignored,
        |config, value| config.picker.ignored = value,
    );

    view! {
        SettingsGroup {
            SettingsGroupLabel {"The modal"}
            SettingsItem(label = "Show a preview beside the list") {
                Switch(class = "config__switch", checked = preview, {..use_settings_item_attrs()})
            }
            SettingsItem(label = "How many rows at once") {
                Number(value = rows, min = 20.0, max = 2000.0, step = 20.0, unit = "")
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"Searching"}
            SettingsItem(
                label = "Smart case",
                description = "A search with no capitals in it matches either case."
            ) {
                Switch(class = "config__switch", checked = smart_case, {..use_settings_item_attrs()})
            }
            SettingsItem(label = "Look at files beginning with a dot") {
                Switch(class = "config__switch", checked = hidden, {..use_settings_item_attrs()})
            }
            SettingsItem(label = "Look inside files git ignores") {
                Switch(class = "config__switch", checked = ignored, {..use_settings_item_attrs()})
            }
        }
    }
}
