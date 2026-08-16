//! The panel: the page list, and the page that is showing.

use super::*;
use zdt_icons::{self as icons, IconProps};
use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui::prelude::*;

/// The whole page.
#[component]
pub fn ConfigPanel() -> impl IntoView {
    view! {
        Settings(
            class = "config",
            default_page = "appearance",
            label = "Settings"
        ) {
            // The glyph is decoration and the word is the name, so the icons are unlabelled: a
            // reader told "palette, Appearance" has been told the same thing twice.
            SettingsPages(label = "Pages") {
                SettingsPage(value = "appearance") {
                    Icon(icon = icons::PALETTE, class = "config__page-icon")
                    "Appearance"
                }
                SettingsPage(value = "editor") {
                    Icon(icon = icons::PENCIL, class = "config__page-icon")
                    "Editor"
                }
                SettingsPage(value = "language") {
                    Icon(icon = icons::LANGUAGES, class = "config__page-icon")
                    "Language"
                }
                SettingsPage(value = "tree") {
                    Icon(icon = icons::FOLDER_TREE, class = "config__page-icon")
                    "File tree"
                }
                SettingsPage(value = "picker") {
                    Icon(icon = icons::SEARCH, class = "config__page-icon")
                    "Pickers"
                }
                SettingsPage(value = "terminal") {
                    Icon(icon = icons::TERMINAL, class = "config__page-icon")
                    "Terminal"
                }
                SettingsPage(value = "keys") {
                    Icon(icon = icons::KEYBOARD, class = "config__page-icon")
                    "Keys"
                }
            }

            // Each pane reaches for the settings itself. A pane's children are rebuilt whenever
            // it is shown again, so anything captured here would have to survive being moved out
            // of a closure that runs more than once.
            SettingsPane(value = "appearance") { Appearance() }
            SettingsPane(value = "editor") { Editing() }
            SettingsPane(value = "language") { Language() }
            SettingsPane(value = "tree") { Tree() }
            SettingsPane(value = "picker") { Pickers() }
            SettingsPane(value = "terminal") { Terminal() }
            SettingsPane(value = "keys") { Keys() }
        }
    }
}
