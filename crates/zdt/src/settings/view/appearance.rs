//! How it looks.

use super::*;
use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui::prelude::*;

/// How the interface looks.
#[component]
pub(crate) fn Appearance() -> impl IntoView {
    let settings = crate::settings::use_settings();
    // The themes the editor knows about: the ones compiled in, and whatever is in the themes
    // directory. Read once. A theme appearing while the panel is open is what the file watcher is
    // for, and a directory listing per frame is not.
    // Stored, because a select's content is built inside a closure that runs again every time the
    // list is opened.
    let themes = StoredValue::new_local(zdt_core::theme::theme_names(
        settings
            .paths()
            .map(zdt_core::config::Paths::themes)
            .as_deref(),
    ));

    let theme = bound(
        &settings,
        |config| config.ui.theme.clone(),
        |config, value| config.ui.theme = value,
    );
    let scheme = bound(
        &settings,
        |config| scheme_name(config.ui.scheme).to_owned(),
        |config, value| config.ui.scheme = scheme_of(&value),
    );
    let font = bound(
        &settings,
        |config| config.ui.font.clone(),
        |config, value| config.ui.font = value,
    );
    let size = number(
        &settings,
        |config| config.ui.font_size,
        |config, value| config.ui.font_size = value as f32,
    );
    let weight = number(
        &settings,
        |config| config.ui.font_weight,
        |config, value| config.ui.font_weight = value as u16,
    );
    let decorations = bound(
        &settings,
        |config| config.ui.client_side_decorations,
        |config, value| config.ui.client_side_decorations = value,
    );
    let notifications = bound(
        &settings,
        |config| config.ui.notifications,
        |config, value| config.ui.notifications = value,
    );
    let timeout = number(
        &settings,
        |config| config.ui.notification_timeout as u32,
        |config, value| config.ui.notification_timeout = value as u64,
    );
    let whichkey = number(
        &settings,
        |config| config.ui.whichkey_delay as u32,
        |config, value| config.ui.whichkey_delay = value as u64,
    );

    view! {
        SettingsGroup {
            SettingsGroupLabel {"Theme"}
            SettingsItem(label = "Theme") {
                // A native chooser, and not the overlay one. A settings row wants a list of a
                // dozen names and a value. The overlay `Select` brings a portal, a registered
                // listbox and its own focus scope, and each of those is something else to be
                // wrong on a page that is already inside a floating panel.
                NativeSelect(
                    class = "config__select",
                    value = theme,
                    size = NativeSelectSize::Sm,
                    {..use_settings_item_attrs()}
                ) {
                    {move || themes
                        .get_value()
                        .into_iter()
                        .map(|name| {
                            use zdt_view::Erase;
                            view! {
                                NativeSelectOption(value = name.clone()) {{name}}
                            }
                            .any()
                        })
                        .collect::<Vec<_>>()}
                }
            }
            SettingsItem(
                label = "Surface",
                description = "Follow the desktop, or pin one."
            ) {
                NativeSelect(
                    class = "config__select",
                    value = scheme,
                    size = NativeSelectSize::Sm,
                    {..use_settings_item_attrs()}
                ) {
                    NativeSelectOption(value = "dark") {"Dark"}
                    NativeSelectOption(value = "light") {"Light"}
                    NativeSelectOption(value = "system") {"Follow the desktop"}
                }
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"Type"}
            SettingsItem(label = "Interface font") {
                Input(class = "config__input", value = font, {..use_settings_item_attrs()})
            }
            SettingsItem(label = "Interface size") {
                Number(value = size, min = 8.0, max = 24.0, step = 1.0, unit = "px")
            }
            SettingsItem(
                label = "Interface weight",
                description = "400 is regular, 700 is bold. A font with no such weight is drawn \
                               in the nearest one it has."
            ) {
                Number(value = weight, min = 100.0, max = 900.0, step = 100.0, unit = "")
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"Window"}
            SettingsItem(
                label = "Draw the window frame",
                description = "Off puts the desktop's own title bar back. Takes a restart."
            ) {
                Switch(class = "config__switch", checked = decorations, {..use_settings_item_attrs()})
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"Announcements"}
            SettingsGroupDescription {
                "What the editor says about things nobody asked it about: a language server \
                 starting, a file that would not read."
            }
            SettingsItem(label = "Show announcements") {
                Switch(class = "config__switch", checked = notifications, {..use_settings_item_attrs()})
            }
            SettingsItem(
                label = "How long they stay",
                description = "Zero keeps them until they are dismissed."
            ) {
                Number(value = timeout, min = 0.0, max = 20000.0, step = 500.0, unit = "ms")
            }
            SettingsItem(
                label = "Which-key delay",
                description = "How long a part-typed sequence sits before the hints appear."
            ) {
                Number(value = whichkey, min = 0.0, max = 2000.0, step = 50.0, unit = "ms")
            }
        }
    }
}
