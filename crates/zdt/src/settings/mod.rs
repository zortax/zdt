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
    pending: RefCell<Option<zdt_view::Pending>>,
    /// The clock the write is debounced on.
    ///
    /// The settings are older than any one window, so this is a clock of their own that a window
    /// lends its engine to. See [`zdt_view::Clock`].
    clock: zdt_view::Clock,
    /// Where a failed write is announced.
    ///
    /// An announcer, because the write happens in a debounce timer, a context looked up there is
    /// gone, and the window that was open when the change was made may not be the one that is
    /// open when the write fails. See `tests/context.rs`.
    announcer: crate::notify::Announcer,
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
                clock: zdt_view::Clock::new(),
                announcer: crate::notify::Announcer::new(),
            }),
        }
    }

    /// The clock the write debounce runs on.
    ///
    /// A window binds its own engine to this while it is open. See [`zdt_view::Clock`].
    #[must_use]
    pub fn clock(&self) -> &zdt_view::Clock {
        &self.inner.clock
    }

    /// Where a failed write is announced.
    #[must_use]
    pub fn announcer(&self) -> &crate::notify::Announcer {
        &self.inner.announcer
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

    /// Says which window's stack a failed write should be announced on.
    ///
    /// Set from a window once its announcements exist. The settings are read before them, because
    /// the settings say whether announcements are wanted at all.
    pub fn announce_through(&self, notify: crate::notify::Notify) {
        self.inner.announcer.bind(notify);
    }

    /// Changes one thing and writes the file, which the settings panel does.
    ///
    /// The write is debounced, so a dragged slider is one write and not one per pixel.
    pub fn edit(&self, change: impl FnOnce(&mut Config)) {
        self.update(change);
        self.persist_soon();
    }

    /// Writes the settings out, after a pause.
    ///
    /// With no window lending a clock the write happens at once, because the alternative is
    /// holding somebody's change until one opens.
    pub fn persist_soon(&self) {
        let settings = self.clone();
        let handle = self.inner.clock.after(WRITE_DEBOUNCE, move || {
            settings.inner.pending.borrow_mut().take();
            settings.persist_now();
        });
        // Replacing the handle cancels the one before it, which is the debounce.
        *self.inner.pending.borrow_mut() = Some(handle);
    }

    /// Writes whatever is waiting, now. What closing a window does.
    pub fn flush(&self) {
        if self.inner.pending.borrow_mut().take().is_some() {
            self.persist_now();
        }
    }

    /// Writes the settings out, saying so if it fails.
    fn persist_now(&self) {
        if let Err(error) = self.persist() {
            let said = error.to_string();
            // Logged as well as announced: a window may open long after this, and a configuration
            // that would not write is worth finding in the log either way.
            tracing::warn!("could not write config.toml: {said}");
            self.inner
                .announcer
                .fail("could not write config.toml", Some(said));
        }
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
    ///
    /// Written to disk: a colour scheme is a lasting choice, and one that came back different
    /// after a restart would read as a bug.
    pub fn toggle_scheme(&self) {
        self.edit(|config| {
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
