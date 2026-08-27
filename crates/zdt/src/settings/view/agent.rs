//! The agent surface, and how much its threads say.

use super::*;
use zdt_agent::mode::RuntimeMode;
use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui::prelude::*;

/// How the agent behaves, and how its threads read.
// The option list is built inside a closure the select runs again every time it opens, so the
// one it is handed here is not redundant.
#[allow(clippy::redundant_closure)]
#[component]
pub(crate) fn Agent() -> impl IntoView {
    use zdt_view::Erase;

    let settings = crate::settings::use_settings();

    let activity = bound(
        &settings,
        |config| activity_name(config.agent.activity).to_owned(),
        |config, value| config.agent.activity = activity_of(&value),
    );
    let stream = bound(
        &settings,
        |config| config.agent.stream,
        |config, value| config.agent.stream = value,
    );
    let mode = bound(
        &settings,
        |config| config.agent.default_mode.clone(),
        |config, value| config.agent.default_mode = value,
    );
    let titles = bound(
        &settings,
        |config| config.agent.titles,
        |config, value| config.agent.titles = value,
    );
    let open = bound(
        &settings,
        |config| config.agent.open,
        |config, value| config.agent.open = value,
    );

    view! {
        SettingsGroup {
            SettingsGroupLabel {"The thread"}
            SettingsItem(
                label = "Tool calls and thoughts",
                description = "Grouped folds a run of them into one card that counts what it \
                               did, and opens on a press. The full log gives every call and \
                               every thought a line of its own."
            ) {
                NativeSelect(
                    class = "config__select",
                    value = activity,
                    size = NativeSelectSize::Sm,
                    {..use_settings_item_attrs()}
                ) {
                    NativeSelectOption(value = "grouped") {"Grouped into cards"}
                    NativeSelectOption(value = "verbose") {"The full log"}
                }
            }
            SettingsItem(
                label = "Show prose while it streams",
                description = "Off holds each message back until it is done, so half-arrived \
                               markdown is never drawn."
            ) {
                Switch(class = "config__switch", checked = stream, {..use_settings_item_attrs()})
            }
            SettingsItem(
                label = "Name threads automatically",
                description = "A thread names itself after its first turn."
            ) {
                Switch(class = "config__switch", checked = titles, {..use_settings_item_attrs()})
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"New threads"}
            SettingsItem(
                label = "How much a new agent may do unasked",
                description = "What a thread starts in. Each thread can be moved from there \
                               without changing this."
            ) {
                NativeSelect(
                    class = "config__select",
                    value = mode,
                    size = NativeSelectSize::Sm,
                    {..use_settings_item_attrs()}
                ) {
                    {move || RuntimeMode::CHOICES
                        .into_iter()
                        .map(|mode| view! {
                            NativeSelectOption(value = mode.word()) {{mode.label()}}
                        }
                        .any())
                        .collect::<Vec<_>>()}
                }
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"The sidebar"}
            SettingsItem(label = "Open the thread list with the window") {
                Switch(class = "config__switch", checked = open, {..use_settings_item_attrs()})
            }
        }
    }
}
