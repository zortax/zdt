//! The settings, as the interface reads them.
//!
//! One signal holding the whole configuration. Everything that follows a setting reads it from
//! here, so a change on disk is a change on screen with nothing to keep in step. `<Leader>u*`
//! writes into the same place, and there is no second copy of the truth.

pub mod view;

use std::cell::RefCell;
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
    /// Exactly what this editor last wrote to the settings file.
    ///
    /// A write to the configuration directory is a change the watcher reports, and the watcher
    /// cannot tell one made here from one made in another window. Without this, saving from the
    /// settings panel would be a write, a report, a read, and a "configuration reloaded" toast
    /// for every keystroke somebody spent on a slider.
    ///
    /// Held as the text, and never as a flag. A flag would have to be cleared on a timer, and a
    /// timer that fired late would suppress somebody *else's* change.
    stamp: RefCell<Option<String>>,
    /// What is waiting to be written, so that dragging a slider is one write.
    pending: RefCell<Option<zgui::view::time::TimeoutHandle>>,
    /// The window's clock, taken once where there certainly is one.
    timers: Option<zgui::view::time::Timers>,
    /// Where a failed write is announced.
    ///
    /// Taken once, because the write happens in a debounce timer and a context looked up there
    /// is gone. See `tests/context.rs`.
    notify: RefCell<Option<crate::notify::Notify>>,
}

/// How long after the last change the settings are written.
///
/// Long enough that a dragged slider is one write, and short enough that letting go and switching
/// to another window finds the file already changed.
const WRITE_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(400);

impl Settings {
    /// The settings in `config`, which came from `paths`.
    #[must_use]
    pub fn new(config: Config, paths: Option<Paths>) -> Self {
        Self {
            inner: Rc::new(Inner {
                config: RwSignal::new_local(config),
                paths,
                stamp: RefCell::new(None),
                pending: RefCell::new(None),
                timers: zgui::view::time::Timers::current(),
                notify: RefCell::new(None),
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

    /// Says where a failed write should be announced.
    ///
    /// Set from the root once the announcements exist. The settings are read before them,
    /// because the settings say whether announcements are wanted at all.
    pub fn announce_through(&self, notify: crate::notify::Notify) {
        *self.inner.notify.borrow_mut() = Some(notify);
    }

    /// Changes one thing and writes the file, which the settings panel does.
    ///
    /// The write is debounced, so a dragged slider is one write and not one per pixel.
    pub fn edit(&self, change: impl FnOnce(&mut Config)) {
        self.update(change);
        self.persist_soon();
    }

    /// Writes the settings out, after a pause.
    pub fn persist_soon(&self) {
        let Some(timers) = self.inner.timers.clone() else {
            // No clock: a test, or a window that has gone. Write now.
            let _ = self.persist();
            return;
        };
        let settings = self.clone();
        let handle = timers.set_timeout(WRITE_DEBOUNCE, move || {
            settings.inner.pending.borrow_mut().take();
            if let Err(error) = settings.persist() {
                let said = error.to_string();
                match settings.inner.notify.borrow().as_ref() {
                    Some(notify) => notify.fail("could not write config.toml", Some(said)),
                    None => tracing::warn!("could not write config.toml: {said}"),
                }
            }
        });
        // Replacing the handle cancels the one before it, which is the debounce.
        *self.inner.pending.borrow_mut() = Some(handle);
    }

    /// Writes the settings out as a list of disagreements with the defaults.
    ///
    /// Only what differs, so that changing one thing in the panel does not turn a three-line file
    /// somebody wrote by hand into two hundred lines they did not.
    ///
    /// # Errors
    ///
    /// When the file cannot be written. A configuration directory that is not there is made.
    pub fn persist(&self) -> Result<(), zdt_core::config::ConfigError> {
        let Some(paths) = self.inner.paths.as_ref() else {
            return Ok(());
        };
        let text = self
            .inner
            .config
            .with_untracked(zdt_core::config::write_diff);
        // Stamped *before* the write, because the watcher can report it while this call is still
        // returning.
        *self.inner.stamp.borrow_mut() = Some(text.clone());
        zdt_core::config::write_atomically(&paths.config(), &text)
    }

    /// Whether `text` is exactly what this editor last wrote.
    ///
    /// What the reload uses to tell its own write apart from somebody else's. The stamp is taken
    /// and cleared by the read, so a second change to the same bytes from another window is
    /// applied.
    #[must_use]
    pub fn wrote(&self, text: Option<&str>) -> bool {
        let Some(text) = text else {
            return false;
        };
        let mut stamp = self.inner.stamp.borrow_mut();
        if stamp.as_deref() == Some(text) {
            *stamp = None;
            return true;
        }
        false
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
/// If none were provided above this component. That is a wiring mistake, and nothing can carry on
/// from it.
#[must_use]
pub fn use_settings() -> Settings {
    zgui::reactive::use_local_context::<Settings>().expect("settings are provided at the root")
}
