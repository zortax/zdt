//! The terminal.

use super::*;
use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui::prelude::*;

/// How terminals are started.
#[component]
pub(crate) fn Terminal() -> impl IntoView {
    let settings = crate::settings::use_settings();
    let shell = bound(
        &settings,
        |config| config.terminal.shell.clone(),
        |config, value| config.terminal.shell = value,
    );
    let width = number(
        &settings,
        |config| (config.terminal.float_width * 100.0).round() as u32,
        |config, value| config.terminal.float_width = (value / 100.0) as f32,
    );
    let height = number(
        &settings,
        |config| (config.terminal.float_height * 100.0).round() as u32,
        |config, value| config.terminal.float_height = (value / 100.0) as f32,
    );
    let scrollback = number(
        &settings,
        |config| config.terminal.scrollback as u32,
        |config, value| config.terminal.scrollback = value as usize,
    );

    view! {
        SettingsGroup {
            SettingsGroupLabel {"The program"}
            SettingsItem(
                label = "Shell",
                description = "Left empty, whatever $SHELL says is used."
            ) {
                Input(class = "config__input", value = shell, placeholder = "$SHELL", {..use_settings_item_attrs()})
            }
            SettingsItem(label = "How many lines of scrollback") {
                Number(value = scrollback, min = 100.0, max = 100000.0, step = 1000.0, unit = "")
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"The floating one"}
            SettingsItem(label = "How wide") {
                Number(value = width, min = 30.0, max = 100.0, step = 5.0, unit = "%")
            }
            SettingsItem(label = "How tall") {
                Number(value = height, min = 30.0, max = 100.0, step = 5.0, unit = "%")
            }
        }
    }
}
