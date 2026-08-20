//! Everything the whole application shares.
//!
//! One configuration directory, one set of settings, one keymap, one theme — however many windows
//! and however many sessions there are. Built once, above every window, and read from inside all
//! of them.
//!
//! The tier below this is the session ([`crate::session`]), which is one directory's work. The
//! tier below that is the window, which is one view of one session.
//!
//! # Why this is above the windows and not inside one
//!
//! [`zgui::app::App::with_context`] runs its setup in the application's own scope, and every
//! window's scope is a child of it. A value provided there is therefore visible from inside every
//! window and outlives all of them, which is exactly what a setting is.
//!
//! Two things that look as though they belong here do not. A style sheet is installed into *a
//! document*, so the sheets follow the signals here but are installed once per window by
//! [`install_window_styles`]. And a clock belongs to a window's frame loop, so anything here that
//! needs one holds a [`zdt_view::Clock`] that a window lends its engine to.

use zdt_core::ThemeSource;
use zdt_core::config::{Paths, Scheme};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RenderEffect, RwSignal, Signal};
use zgui_ui_tokens::ColorScheme;

use crate::keymaps::Keymaps;
use crate::settings::Settings;

/// What every window and every session reads.
///
/// Cloning one is cloning a handle.
#[derive(Clone)]
pub struct Global {
    /// Where the configuration lives, when it lives anywhere.
    paths: Option<Paths>,
    settings: Settings,
    keymaps: Keymaps,
    /// Which theme is in force. A signal, because a reload changes it under everything.
    theme: RwSignal<ThemeSource, LocalStorage>,
    /// What the configuration says the settings would not read.
    problem: Option<String>,
}

impl Global {
    /// Where the configuration lives.
    #[must_use]
    pub fn paths(&self) -> Option<&Paths> {
        self.paths.as_ref()
    }

    /// The settings.
    #[must_use]
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// What every key means.
    #[must_use]
    pub fn keymaps(&self) -> &Keymaps {
        &self.keymaps
    }

    /// Which theme is in force.
    #[must_use]
    pub fn theme(&self) -> RwSignal<ThemeSource, LocalStorage> {
        self.theme
    }

    /// Which surface the theme is presented on.
    #[must_use]
    pub fn scheme(&self) -> Signal<ColorScheme, LocalStorage> {
        let settings = self.settings.clone();
        Signal::derive_local(move || match settings.with(|config| config.ui.scheme) {
            Scheme::Light => ColorScheme::Light,
            Scheme::Dark => ColorScheme::Dark,
            Scheme::System => ColorScheme::System,
        })
    }

    /// What the tree should show, as the settings say.
    #[must_use]
    pub fn tree_filter(&self) -> zdt_core::tree::Filter {
        self.settings
            .with_untracked(|config| zdt_core::tree::Filter {
                hidden: config.tree.hidden,
                ignored: config.tree.ignored,
            })
    }

    /// What went wrong reading the configuration, for the first window to announce.
    ///
    /// Taken once. The second window must not repeat it.
    #[must_use]
    pub fn take_problem(&mut self) -> Option<String> {
        self.problem.take()
    }
}

/// Builds everything shared and publishes it.
///
/// Called from [`zgui::app::App::with_context`], which is the one place above every window.
///
/// The watcher it starts is deliberately never given back: it is held by the application's scope
/// and stops when the application does, which is exactly as long as the configuration is worth
/// watching.
pub fn install() -> Global {
    let paths = Paths::discover();
    let (settings, problem) = Settings::load(paths.clone());
    let keymaps = Keymaps::new();

    // A person's own keys, read after the shipped ones so a row in theirs replaces the shipped row
    // for the same keys.
    if let Some(paths) = paths.as_ref() {
        apply_keymap(&keymaps, settings.announcer(), paths, &settings);
    }
    apply_tree_keymap(&keymaps, settings.announcer(), paths.as_ref());
    apply_all_overlays(&keymaps, settings.announcer(), paths.as_ref());

    let theme: RwSignal<ThemeSource, LocalStorage> =
        RwSignal::new_local(read_theme(&settings, paths.as_ref()));

    let global = Global {
        paths: paths.clone(),
        settings: settings.clone(),
        keymaps: keymaps.clone(),
        theme,
        problem,
    };

    // The theme follows the settings, which follow the files on disk.
    //
    // The name is what is subscribed to, and never the whole theme: reading a theme is reading
    // two files off the disk, and it must happen because the *name* changed rather than because
    // anything else in the settings did.
    let following = {
        let (settings, paths) = (settings.clone(), paths.clone());
        RenderEffect::new(move |previous: Option<String>| {
            let name = settings.with(|config| config.ui.theme.clone());
            if previous.as_ref() != Some(&name) {
                theme.set(read_theme(&settings, paths.as_ref()));
            }
            name
        })
    };
    std::mem::forget(following);

    // What a change on disk does. Held for the application's life.
    if let Some(paths) = paths.as_ref() {
        let watcher = {
            let (global, held) = (global.clone(), paths.clone());
            crate::reload::watch(paths, move || reload(&global, &held))
        };
        std::mem::forget(watcher);
    }

    zgui::reactive::provide_local_context(global.clone());
    crate::settings::provide(settings);
    crate::keymaps::provide(keymaps);
    global
}

/// What the whole application shares, from inside a component.
///
/// # Panics
///
/// If none was installed above this window. That is a wiring mistake, and nothing can carry on
/// from it.
#[must_use]
pub fn use_global() -> Global {
    zgui::reactive::use_local_context::<Global>().expect("the global tier is installed at the root")
}

/// Installs the two style sheets that belong to a document.
///
/// Called once per window. A sheet is installed into the window it is asked for from, so these
/// cannot live in the application's scope even though what they are made of does.
pub fn install_window_styles(global: &Global) {
    // The settings that are style, in the cascade between the theme and a person's own sheet.
    let styling = {
        let settings = global.settings.clone();
        RenderEffect::new(move |_| {
            let css = settings.with(crate::app::theme::settings_sheet);
            zgui::view::sheet::install_stylesheet(crate::app::theme::SETTINGS_SHEET, &css);
        })
    };
    on_cleanup_local(move || drop(styling));

    // A person's own sheet, last of the three.
    if let Some(paths) = global.paths.as_ref() {
        crate::app::theme::install_user_css(
            zdt_core::config::read_optional(&paths.user_css()).as_deref(),
        );
    }
}

/// The theme the settings name, or the one the editor falls back to.
///
/// Untracked: this reads files, and the effect that wants it subscribes to the *name* so that it
/// runs when the name changes and not when anything else in the settings does.
fn read_theme(settings: &Settings, paths: Option<&Paths>) -> ThemeSource {
    let name = settings.with_untracked(|config| config.ui.theme.clone());
    let directory = paths.map(Paths::themes);
    zdt_core::theme::resolve_theme(directory.as_deref(), &name).unwrap_or_else(|| {
        tracing::warn!("no theme called {name}; using the built-in one");
        crate::app::theme::fallback()
    })
}

/// Everything a change on disk brings, put where the interface reads it.
///
/// The files are read on a worker; only the writing happens here. A settings file that does not
/// read leaves the old settings in place and says so, because half-applied configuration is worse
/// than none.
fn reload(global: &Global, paths: &Paths) {
    let (global, paths) = (global.clone(), paths.clone());

    let task = zgui::task::spawn_local(async move {
        let reading = paths.clone();
        let reloaded = zgui::task::blocking(move || crate::reload::read(&reading)).await;
        let settings = &global.settings;
        let announcer = settings.announcer();

        // What this editor wrote itself, coming back around through the watcher. Applying it
        // would be applying what is already applied, and saying so would be announcing somebody's
        // own keystroke back at them.
        if settings.wrote(reloaded.config_text.as_deref()) {
            return;
        }

        for problem in &reloaded.problems {
            announcer.fail("configuration", Some(problem.clone()));
        }
        if let Some(config) = reloaded.config {
            settings.replace(config);
        }

        // The keymap is rebuilt from the shipped one, and never layered onto what is already
        // there. A row somebody took out of their file has to come back.
        global.keymaps.reset();
        if let Some(text) = reloaded.keymap
            && let Err(problems) = global.keymaps.merge(&text, leaders_from(settings))
        {
            announcer.fail("keymap.toml", Some(problems.join("; ")));
        }

        apply_tree_keymap(&global.keymaps, announcer, Some(&paths));
        apply_all_overlays(&global.keymaps, announcer, Some(&paths));
        global.theme.set(read_theme(settings, Some(&paths)));
        crate::app::theme::install_user_css(reloaded.user_css.as_deref());

        if reloaded.problems.is_empty() {
            announcer.say("configuration reloaded");
        }
    });
    // The task belongs to the application's scope and is cancelled with it.
    std::mem::forget(task);
}

/// The file tree's keys: the shipped ones, then a person's own on top.
fn apply_tree_keymap(
    keymaps: &Keymaps,
    announcer: &crate::notify::Announcer,
    paths: Option<&Paths>,
) {
    let theirs = paths.and_then(|paths| zdt_core::config::read_optional(&paths.tree_keymap()));
    if let Err(problems) =
        keymaps.load_overlay("tree", crate::assets::TREE_KEYMAP, theirs.as_deref())
    {
        announcer.fail("keymap-tree.toml", Some(problems.join("; ")));
    }
}

/// A region's own keys: the shipped ones, then a person's own on top.
///
/// Every overlay has the same shape: a shipped file, and an optional one beside it in the
/// configuration directory. So one function loads them all.
fn apply_overlay(
    keymaps: &Keymaps,
    announcer: &crate::notify::Announcer,
    paths: Option<&Paths>,
    region: &str,
    shipped: &str,
    file: &str,
) {
    let theirs = paths.and_then(|paths| zdt_core::config::read_optional(&paths.root.join(file)));
    if let Err(problems) = keymaps.load_overlay(region, shipped, theirs.as_deref()) {
        announcer.fail(file.to_owned(), Some(problems.join("; ")));
    }
}

/// Every region's keymap overlay, loaded.
///
/// One call site for all of them, so a region added later is one row here. Three places that have
/// to agree would be three places to forget.
fn apply_all_overlays(
    keymaps: &Keymaps,
    announcer: &crate::notify::Announcer,
    paths: Option<&Paths>,
) {
    for (region, shipped, file) in crate::assets::OVERLAYS {
        apply_overlay(keymaps, announcer, paths, region, shipped, file);
    }
}

/// Reads a person's keymap on top of the shipped one, saying what did not read.
fn apply_keymap(
    keymaps: &Keymaps,
    announcer: &crate::notify::Announcer,
    paths: &Paths,
    settings: &Settings,
) {
    let Some(text) = zdt_core::config::read_optional(&paths.keymap()) else {
        return;
    };
    if let Err(problems) = keymaps.merge(&text, leaders_from(settings)) {
        announcer.fail("keymap.toml", Some(problems.join("; ")));
    }
}

/// What `<Leader>` and `<LocalLeader>` stand for, as the settings say.
fn leaders_from(settings: &Settings) -> zdt_vim::Leaders {
    let (leader, local) = settings
        .with_untracked(|config| (config.keys.leader.clone(), config.keys.local_leader.clone()));
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
