//! What the window contains.
//!
//! Three rows inside the frame: the combined header, the panes, and the status line. Everything
//! below reads the workspace, the settings and the modal layer, all of which are provided here and
//! nowhere else.

use std::path::PathBuf;

use zdt_core::config::{Paths, Scheme};
use zdt_core::{Project, ThemeSource};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RenderEffect, RwSignal};
use zgui::{component, view};
use zgui_ui_tokens::ColorScheme;

use crate::settings::Settings;
use crate::ui::chrome::ChromeProps;
use crate::ui::frame::FrameProps;
use crate::ui::panes::PanesProps;
use crate::ui::statusline::StatusLineProps;
use crate::ui::theme::{ZdtThemeProps, fallback};
use crate::ui::whichkey::WhichKeyProps;
use crate::vim::Vim;
use crate::workspace::{self, Workspace};

/// The application.
#[component]
pub fn Root(
    /// The directory the editor was opened on.
    project: Project,
    /// The files named on the command line.
    files: Vec<PathBuf>,
) -> impl IntoView {
    let paths = Paths::discover();
    let (settings, problem) = Settings::load(paths.clone());
    crate::settings::provide(settings.clone());

    let space = Workspace::new(project);
    workspace::provide(space.clone());
    if let Some(problem) = problem {
        space.complain(problem);
    }

    let vim = Vim::new(space.clone(), settings.clone());
    zgui::reactive::provide_local_context(vim.clone());

    // A person's own keymap, read after the shipped one so a row in it replaces the shipped row
    // for the same keys.
    if let Some(paths) = paths.as_ref() {
        apply_keymap(&vim, &space, paths, &settings);
    }

    // The theme follows the settings, and both follow the files on disk.
    let theme: RwSignal<ThemeSource, LocalStorage> = RwSignal::new_local(read_theme(&settings));
    let following = {
        let settings = settings.clone();
        RenderEffect::new(move |_| {
            let next = read_theme(&settings);
            if theme.with_untracked(|held| held.name != next.name) {
                theme.set(next);
            }
        })
    };
    on_cleanup_local(move || drop(following));

    let scheme = {
        let settings = settings.clone();
        Signal::derive_local(move || match settings.with(|config| config.ui.scheme) {
            Scheme::Light => ColorScheme::Light,
            Scheme::Dark => ColorScheme::Dark,
            Scheme::System => ColorScheme::System,
        })
    };

    // The settings that are style rather than behaviour, in the cascade between the theme and a
    // person's own sheet.
    let styling = {
        let settings = settings.clone();
        RenderEffect::new(move |_| {
            let css = settings.with(crate::ui::theme::settings_sheet);
            install_stylesheet(crate::ui::theme::SETTINGS_SHEET, &css);
        })
    };
    on_cleanup_local(move || drop(styling));

    // A person's own sheet, last of the three.
    if let Some(paths) = paths.as_ref() {
        crate::ui::theme::install_user_css(
            zdt_core::config::read_optional(&paths.user_css()).as_deref(),
        );
    }

    // What a change on disk does. Held for the window's life; dropping it stops the watching.
    let watcher = paths.as_ref().and_then(|paths| {
        let (settings, space, vim, held) =
            (settings.clone(), space.clone(), vim.clone(), paths.clone());
        crate::reload::watch(paths, move || {
            reload(&settings, &space, &vim, &held, theme);
        })
    });
    on_cleanup_local(move || drop(watcher));

    for file in files {
        crate::files::open_argument(&space, &file);
    }

    view! {
        ZdtTheme(theme = theme, scheme = scheme) {
            Frame {
                Chrome()
                Panes()
                WhichKey()
                StatusLine()
            }
        }
    }
}

/// The theme the settings name, or the one the editor falls back to.
fn read_theme(settings: &Settings) -> ThemeSource {
    let name = settings.with(|config| config.ui.theme.clone());
    let directory = settings.paths().map(zdt_core::config::Paths::themes);
    zdt_core::theme::resolve_theme(directory.as_deref(), &name).unwrap_or_else(|| {
        tracing::warn!("no theme called {name}; using the built-in one");
        fallback()
    })
}

/// Reads a person's keymap on top of the shipped one, saying what did not read.
fn apply_keymap(vim: &Vim, space: &Workspace, paths: &Paths, settings: &Settings) {
    let Some(text) = zdt_core::config::read_optional(&paths.keymap()) else {
        return;
    };
    let leaders = leaders_from(settings);
    if let Err(problems) = vim.merge_keymap(&text, leaders) {
        space.complain(format!("keymap.toml: {}", problems.join("; ")));
    }
}

/// What `<Leader>` and `<LocalLeader>` stand for, as the settings say.
fn leaders_from(settings: &Settings) -> zdt_vim::Leaders {
    let (leader, local) = settings.with(|config| {
        (
            config.keys.leader.clone(),
            config.keys.local_leader.clone(),
        )
    });
    let default = zdt_vim::Leaders::default();
    let one = |text: &str, fallback| {
        zdt_vim::notation::parse(text, default)
            .ok()
            .and_then(|chords| chords.first().copied())
            .unwrap_or(fallback)
    };
    zdt_vim::Leaders {
        leader: one(&leader, default.leader),
        local: one(&local, default.local),
    }
}

/// Everything a change on disk brings, put where the interface reads it.
///
/// The files are read on a worker; only the writing happens here. A settings file that does not
/// read leaves the old settings in place and says so, because half-applied configuration is worse
/// than none.
fn reload(
    settings: &Settings,
    space: &Workspace,
    vim: &Vim,
    paths: &Paths,
    theme: RwSignal<ThemeSource, LocalStorage>,
) {
    let (settings, space, vim, paths) =
        (settings.clone(), space.clone(), vim.clone(), paths.clone());

    let task = zgui::task::spawn_local(async move {
        let reading = paths.clone();
        let reloaded = zgui::task::blocking(move || crate::reload::read(&reading)).await;

        for problem in &reloaded.problems {
            space.complain(problem.clone());
        }
        if let Some(config) = reloaded.config {
            settings.replace(config);
        }

        // The keymap is rebuilt from the shipped one rather than layered onto what is already
        // there: a row somebody took out of their file has to come back.
        vim.reset_keymap();
        if let Some(text) = reloaded.keymap
            && let Err(problems) = vim.merge_keymap(&text, leaders_from(&settings))
        {
            space.complain(format!("keymap.toml: {}", problems.join("; ")));
        }

        theme.set(read_theme(&settings));
        crate::ui::theme::install_user_css(reloaded.user_css.as_deref());

        if reloaded.problems.is_empty() {
            space.say("configuration reloaded");
        }
    });
    // The task belongs to the root's owner and is cancelled with the window.
    std::mem::forget(task);
}
