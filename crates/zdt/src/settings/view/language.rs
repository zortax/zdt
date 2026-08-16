//! Language servers, and what they offer.

use super::*;
use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui::prelude::*;

/// What the language servers do.
#[component]
pub(crate) fn Language() -> impl IntoView {
    let settings = crate::settings::use_settings();
    let enabled = bound(
        &settings,
        |config| config.lsp.enabled,
        |config, value| config.lsp.enabled = value,
    );
    let completion = bound(
        &settings,
        |config| config.editor.completion,
        |config, value| config.editor.completion = value,
    );
    let least = number(
        &settings,
        |config| config.editor.completion_min_chars as u32,
        |config, value| config.editor.completion_min_chars = value as usize,
    );
    let docs = bound(
        &settings,
        |config| config.editor.completion_doc,
        |config, value| config.editor.completion_doc = value,
    );
    let delay = number(
        &settings,
        |config| config.editor.completion_doc_delay as u32,
        |config, value| config.editor.completion_doc_delay = value as u64,
    );
    let highlight = bound(
        &settings,
        |config| config.editor.highlight_symbol,
        |config, value| config.editor.highlight_symbol = value,
    );
    let highlight_delay = number(
        &settings,
        |config| config.editor.highlight_symbol_delay as u32,
        |config, value| config.editor.highlight_symbol_delay = value as u64,
    );
    let format_on_save = bound(
        &settings,
        |config| config.editor.format_on_save,
        |config, value| config.editor.format_on_save = value,
    );

    // Which servers are configured, and which of them are answering for the file on screen. Read
    // only. A server is a dozen fields and a command line, and a panel that edited those badly
    // would be worse than the file that does it well.
    let names = {
        let settings = settings.clone();
        move || {
            settings.with(|config| {
                let mut names: Vec<String> = config.lsp.servers.keys().cloned().collect();
                names.sort_unstable();
                names
            })
        }
    };
    let running = move || {
        zgui::reactive::use_local_context::<crate::language::Language>()
            .and_then(|language| {
                let path = language.current_path()?;
                Some(language.servers_for(&path))
            })
            .unwrap_or_default()
    };

    view! {
        SettingsGroup {
            SettingsGroupLabel {"Language servers"}
            SettingsItem(
                label = "Use language servers",
                description = "Off stops every server and draws no diagnostics."
            ) {
                Switch(class = "config__switch", checked = enabled, {..use_settings_item_attrs()})
            }
            SettingsItem(
                label = "Format when saving",
                description = "Runs the server's formatter before the file is written."
            ) {
                Switch(class = "config__switch", checked = format_on_save, {..use_settings_item_attrs()})
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"Suggestions"}
            SettingsItem(label = "Suggest as you type") {
                Switch(class = "config__switch", checked = completion, {..use_settings_item_attrs()})
            }
            SettingsItem(
                label = "After this many characters",
                description = "One asks as soon as a word starts."
            ) {
                Number(value = least, min = 1.0, max = 5.0, step = 1.0, unit = "")
            }
            SettingsItem(label = "Show documentation beside them") {
                Switch(class = "config__switch", checked = docs, {..use_settings_item_attrs()})
            }
            SettingsItem(
                label = "After resting for",
                description = "Zero opens it at once."
            ) {
                Number(value = delay, min = 0.0, max = 2000.0, step = 50.0, unit = "ms")
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"Under the caret"}
            SettingsItem(
                label = "Mark other uses of the symbol",
                description = "Bands every other place in the file the caret's symbol is used."
            ) {
                Switch(class = "config__switch", checked = highlight, {..use_settings_item_attrs()})
            }
            SettingsItem(label = "After resting for") {
                Number(value = highlight_delay, min = 0.0, max = 2000.0, step = 50.0, unit = "ms")
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"Configured servers"}
            SettingsGroupDescription {
                "Servers are set up in config.toml, where a command line and its arguments belong. \
                 What is running for the file on screen is marked."
            }
            column(class = "config__servers") {
                {move || {
                    let running = running();
                    names()
                        .into_iter()
                        .map(|name| {
                            let on = running.contains(&name);
                            view! {
                                row(
                                    class = "config__server",
                                    attr:data-running = on.then(|| "true".to_owned())
                                ) {
                                    box(class = "config__server-dot") {}
                                    label(class = "nowrap") {{name}}
                                }
                            }
                        })
                        .collect::<Vec<_>>()
                }}
            }
        }
    }
}
