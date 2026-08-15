//! The settings, as the interface reads them.
//!
//! One signal holding the whole configuration. Everything that follows a setting reads it from
//! here, so a change on disk is a change on screen with nothing to keep in step — and `<Leader>u*`
//! writes into the same place rather than into a second copy of the truth.

use std::rc::Rc;

use zdt_core::config::{Config, Paths, Scheme};
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

/// The settings.
///
/// Cloning one is cloning a handle: every clone reads and writes the same configuration.
#[derive(Clone)]
pub struct Settings {
    inner: Rc<Inner>,
}

struct Inner {
    config: RwSignal<Config, LocalStorage>,
    /// Where it was read from, when it was read from anywhere.
    paths: Option<Paths>,
}

impl Settings {
    /// The settings in `config`, which came from `paths`.
    #[must_use]
    pub fn new(config: Config, paths: Option<Paths>) -> Self {
        Self {
            inner: Rc::new(Inner {
                config: RwSignal::new_local(config),
                paths,
            }),
        }
    }

    /// Reads the settings from the configuration directory, saying what went wrong.
    ///
    /// A file that is not there is every default. A file that is there and wrong leaves the
    /// editor on the defaults and says so, because carrying on silently would hide a mistake
    /// somebody made on purpose.
    #[must_use]
    pub fn load(paths: Option<Paths>) -> (Self, Option<String>) {
        let Some(paths) = paths else {
            return (Self::new(Config::default(), None), None);
        };
        match zdt_core::config::load(&paths.config()) {
            Ok(config) => (Self::new(config, Some(paths)), None),
            Err(error) => (
                Self::new(Config::default(), Some(paths)),
                Some(error.to_string()),
            ),
        }
    }

    /// Where the configuration lives, when it lives anywhere.
    #[must_use]
    pub fn paths(&self) -> Option<&Paths> {
        self.inner.paths.as_ref()
    }

    /// The whole configuration. Tracked.
    #[must_use]
    pub fn config(&self) -> Config {
        self.inner.config.get()
    }

    /// The whole configuration, without subscribing.
    #[must_use]
    pub fn config_untracked(&self) -> Config {
        self.inner.config.get_untracked()
    }

    /// Reads one thing out of it. Tracked, and narrower than reading the whole thing.
    #[must_use]
    pub fn with<T>(&self, read: impl FnOnce(&Config) -> T) -> T {
        self.inner.config.with(read)
    }

    /// Reads one thing out of it without subscribing.
    #[must_use]
    pub fn with_untracked<T>(&self, read: impl FnOnce(&Config) -> T) -> T {
        self.inner.config.with_untracked(read)
    }

    /// Puts a whole new configuration in place, which is what a file changing on disk does.
    pub fn replace(&self, config: Config) {
        if self.inner.config.with_untracked(|held| *held != config) {
            self.inner.config.set(config);
        }
    }

    /// Changes one thing, which is what a `<Leader>u*` toggle does.
    ///
    /// The change is not written back to disk: a toggle is for this session, and an editor that
    /// rewrote somebody's configuration file behind them would be a very unwelcome surprise.
    pub fn update(&self, change: impl FnOnce(&mut Config)) {
        self.inner.config.update(change);
    }

    /// Turns the scheme over, which `<Leader>ub` does.
    pub fn toggle_scheme(&self) {
        self.update(|config| {
            config.ui.scheme = match config.ui.scheme {
                Scheme::Light => Scheme::Dark,
                Scheme::Dark | Scheme::System => Scheme::Light,
            };
        });
    }
}

/// Puts the settings where every component can find them.
pub fn provide(settings: Settings) {
    zgui::reactive::provide_local_context(settings);
}

/// The settings, from inside a component.
///
/// # Panics
///
/// If none were provided above this component, which is a wiring mistake rather than a state
/// anything can carry on from.
#[must_use]
pub fn use_settings() -> Settings {
    zgui::reactive::use_local_context::<Settings>().expect("settings are provided at the root")
}
