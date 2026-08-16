//! Quitting, and the `<Leader>u` toggles.

use crate::settings::Settings;
use crate::workspace::Workspace;

/// The application itself.
pub(super) fn run(workspace: &Workspace, leaf: &str) {
    match leaf {
        "quit" => {
            let unsaved = workspace
                .order()
                .into_iter()
                .filter(|id| {
                    workspace
                        .buffer_untracked(*id)
                        .is_some_and(|buffer| buffer.is_dirty())
                })
                .count();
            if unsaved > 0 {
                workspace.complain(format!("{unsaved} buffers have unsaved changes"));
            } else if let Some(windows) =
                zgui::reactive::use_local_context::<zgui::runtime::windows::Windows>()
            {
                windows.quit();
            }
        }
        other => workspace.say(format!("app.{other} is not built yet")),
    }
}

/// The `<Leader>u` toggles.
///
/// Each writes into the settings, and everything that follows one reads them. So a toggle is one
/// line here, and the configuration holds the only copy of the truth.
pub(super) fn toggle(workspace: &Workspace, leaf: &str, args: &zdt_vim::Args) {
    use zdt_core::config::LineNumbers;

    let Some(settings) = zgui::reactive::use_local_context::<Settings>() else {
        return;
    };

    if leaf == "dismiss" {
        workspace.hush();
        if let Some(notify) = crate::notify::use_notify() {
            notify.dismiss_all();
        }
        return;
    }
    // The settings, floating: something opened, changed and closed again, which is a modal.
    if leaf == "settings" {
        if let Some(state) = crate::settings::view::use_config_modal() {
            state.open();
        }
        return;
    }
    // And as a tab, for anybody who wants them beside the file whose behaviour they are changing.
    if leaf == "settings_tab" {
        workspace.open_panel(crate::workspace::BufferKind::Settings);
        return;
    }

    if leaf != "toggle" {
        workspace.say(format!("ui.{leaf} is not built yet"));
        return;
    }

    let setting = args.str("setting").unwrap_or("");
    match setting {
        "scheme" => {
            settings.toggle_scheme();
            let now = settings.with(|config| config.ui.scheme);
            workspace.say(format!("{now:?} theme").to_lowercase());
        }
        "line_numbers" => settings.update(|config| {
            config.editor.line_numbers = match config.editor.line_numbers {
                LineNumbers::None => LineNumbers::Absolute,
                _ => LineNumbers::None,
            };
        }),
        "relative_numbers" => settings.update(|config| {
            config.editor.line_numbers = match config.editor.line_numbers {
                LineNumbers::Relative => LineNumbers::Absolute,
                _ => LineNumbers::Relative,
            };
        }),
        "cursorline" => settings.update(|config| {
            config.editor.cursorline = !config.editor.cursorline;
        }),
        "smooth_scroll" => settings.update(|config| {
            config.editor.smooth_scroll = !config.editor.smooth_scroll;
        }),
        other => workspace.say(format!("there is no `{other}` to toggle yet")),
    }
}
