//! Typing, and what the text does.

use super::*;
use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui::prelude::*;

/// How the editor behaves.
#[component]
pub(crate) fn Editing() -> impl IntoView {
    let settings = crate::settings::use_settings();
    let font = bound(
        &settings,
        |config| config.editor.font.clone(),
        |config, value| config.editor.font = value,
    );
    let size = number(
        &settings,
        |config| config.editor.font_size,
        |config, value| config.editor.font_size = value as f32,
    );
    let weight = number(
        &settings,
        |config| config.editor.font_weight,
        |config, value| config.editor.font_weight = value as u16,
    );
    let numbers = bound(
        &settings,
        |config| line_numbers_name(config.editor.line_numbers).to_owned(),
        |config, value| config.editor.line_numbers = line_numbers_of(&value),
    );
    let tab_size = number(
        &settings,
        |config| config.editor.tab_size,
        |config, value| config.editor.tab_size = value as u32,
    );
    let expand_tab = bound(
        &settings,
        |config| config.editor.expand_tab,
        |config, value| config.editor.expand_tab = value,
    );
    let scrolloff = number(
        &settings,
        |config| config.editor.scrolloff as u32,
        |config, value| config.editor.scrolloff = value as usize,
    );
    let cursorline = bound(
        &settings,
        |config| config.editor.cursorline,
        |config, value| config.editor.cursorline = value,
    );
    let smooth = bound(
        &settings,
        |config| config.editor.smooth_scroll,
        |config, value| config.editor.smooth_scroll = value,
    );
    let threshold = number(
        &settings,
        |config| config.editor.smooth_scroll_min_lines as f32,
        |config, value| config.editor.smooth_scroll_min_lines = value,
    );

    view! {
        SettingsGroup {
            SettingsGroupLabel {"Type"}
            SettingsItem(label = "Editor font") {
                Input(class = "config__input", value = font, {..use_settings_item_attrs()})
            }
            SettingsItem(label = "Editor size") {
                Number(value = size, min = 8.0, max = 32.0, step = 1.0, unit = "px")
            }
            SettingsItem(
                label = "Editor weight",
                description = "400 is regular, 700 is bold."
            ) {
                Number(value = weight, min = 100.0, max = 900.0, step = 100.0, unit = "")
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"Text"}
            SettingsItem(label = "Line numbers") {
                NativeSelect(
                    class = "config__select",
                    value = numbers,
                    size = NativeSelectSize::Sm,
                    {..use_settings_item_attrs()}
                ) {
                    NativeSelectOption(value = "relative") {"Relative"}
                    NativeSelectOption(value = "absolute") {"Absolute"}
                    NativeSelectOption(value = "none") {"None"}
                }
            }
            SettingsItem(label = "Tab width") {
                Number(value = tab_size, min = 1.0, max = 16.0, step = 1.0, unit = "")
            }
            SettingsItem(
                label = "Insert spaces",
                description = "Off inserts a tab character."
            ) {
                Switch(class = "config__switch", checked = expand_tab, {..use_settings_item_attrs()})
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"The view"}
            SettingsItem(
                label = "Keep lines in view",
                description = "How many lines stay between the caret and the edge."
            ) {
                Number(value = scrolloff, min = 0.0, max = 30.0, step = 1.0, unit = "")
            }
            SettingsItem(label = "Tint the caret's line") {
                Switch(class = "config__switch", checked = cursorline, {..use_settings_item_attrs()})
            }
            SettingsItem(label = "Glide when scrolling") {
                Switch(class = "config__switch", checked = smooth, {..use_settings_item_attrs()})
            }
            SettingsItem(
                label = "Jump under",
                description = "How far the view may move and still jump rather than glide."
            ) {
                Number(value = threshold, min = 0.0, max = 20.0, step = 1.0, unit = "lines")
            }
        }
    }
}
