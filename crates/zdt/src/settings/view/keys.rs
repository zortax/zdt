//! The keys.

use super::*;
use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui::prelude::*;

/// Which keys are the leaders.
#[component]
pub(crate) fn Keys() -> impl IntoView {
    let settings = crate::settings::use_settings();
    let leader = bound(
        &settings,
        |config| config.keys.leader.clone(),
        |config, value| config.keys.leader = value,
    );
    let local = bound(
        &settings,
        |config| config.keys.local_leader.clone(),
        |config, value| config.keys.local_leader = value,
    );
    let alphabet = bound(
        &settings,
        |config| config.leap.alphabet.clone(),
        |config, value| config.leap.alphabet = value,
    );

    view! {
        SettingsGroup {
            SettingsGroupLabel {"Leaders"}
            SettingsGroupDescription {
                "Written the way the keymap writes them: <Space>, <C-x>, or a bare character."
            }
            SettingsItem(label = "Leader") {
                Input(class = "config__input", value = leader, {..use_settings_item_attrs()})
            }
            SettingsItem(label = "Local leader") {
                Input(class = "config__input", value = local, {..use_settings_item_attrs()})
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"Leaping"}
            SettingsItem(
                label = "Label alphabet",
                description = "The keys labels are handed out from, in order. The earliest are \
                               the ones the fingers are already on."
            ) {
                Input(class = "config__input", value = alphabet, {..use_settings_item_attrs()})
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"The rest"}
            SettingsGroupDescription {
                "Keys themselves are bound in keymap.toml beside config.toml, read after the map \
                 the editor ships with. A row there replaces the shipped row for the same keys, \
                 and `action = false` removes one."
            }
        }
    }
}
