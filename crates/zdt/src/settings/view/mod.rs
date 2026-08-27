//! The settings, as a page.
//!
//! Every control here is bound straight to [`crate::settings::Settings`], the one signal that
//! everything following a setting reads. A change is live the moment it is made: the theme
//! repaints, the tree re-filters, the editor's font changes. There is nothing to keep in step,
//! because there is no second copy of the truth.
//!
//! # It writes the file too
//!
//! A panel that only changed the running editor would lose its work at the next start. One that
//! rewrote the whole file would turn a three-line configuration somebody wrote by hand into two
//! hundred lines they did not. So [`crate::settings::Settings::persist`] writes only the fields
//! that disagree with the defaults, atomically, four hundred milliseconds after the last change.
//! It stamps what it wrote, so the watcher leaves the editor's own write alone.
//!
//! # Why the library's components are used as they are
//!
//! `zgui-ui` ships the whole settings family: the two columns, the page list with roving focus,
//! the groups, and the rows that name their control for a screen reader. It ships no opinion about
//! this application's density. That lives in `assets/css/settings.css`. The library exposes stable
//! classes for every part, so the compact restyle is a style sheet and nothing is forked.

mod agent;
mod appearance;
mod editing;
mod keys;
mod language;
mod modal;
mod number;
mod panel;
mod pickers;
mod sessions;
mod terminal;
mod tree;

pub use crate::settings::view::modal::{ConfigModal, ConfigModalProps};
pub use crate::settings::view::panel::{ConfigPanel, ConfigPanelProps};

pub(crate) use crate::settings::view::agent::AgentProps;
pub(crate) use crate::settings::view::appearance::AppearanceProps;
pub(crate) use crate::settings::view::editing::EditingProps;
pub(crate) use crate::settings::view::keys::KeysProps;
pub(crate) use crate::settings::view::language::LanguageProps;
pub(crate) use crate::settings::view::number::NumberProps;
pub(crate) use crate::settings::view::pickers::PickersProps;
pub(crate) use crate::settings::view::sessions::SessionsProps;
pub(crate) use crate::settings::view::terminal::TerminalProps;
pub(crate) use crate::settings::view::tree::TreeProps;

use zgui::prelude::*;
use zgui_ui_primitives::Binding;

use crate::settings::Settings as AppSettings;

/// A control's value, read from the settings and written back to them.
///
/// The write goes through [`AppSettings::edit`], which changes the running editor and queues the
/// file. So every control in this panel is live and persistent, and neither is said twice.
fn bound<T: Clone + PartialEq + 'static>(
    settings: &AppSettings,
    read: impl Fn(&zdt_core::Config) -> T + 'static,
    write: impl Fn(&mut zdt_core::Config, T) + 'static,
) -> Binding<T> {
    let reading = settings.clone();
    let writing = settings.clone();
    Binding::controlled(
        Signal::derive_local(move || reading.with(&read)),
        move |value: T| writing.edit(|config| write(config, value)),
    )
}

/// The same, for a number the settings hold as something other than an `f64`.
///
/// Sliders speak `f64`, and the configuration speaks `u32`, `usize` and `f32`. The conversion
/// lives here, and not at eleven call sites.
fn number<T: Copy + PartialEq + 'static>(
    settings: &AppSettings,
    read: impl Fn(&zdt_core::Config) -> T + 'static,
    write: impl Fn(&mut zdt_core::Config, f64) + 'static,
) -> Binding<f64>
where
    f64: From<T>,
{
    let reading = settings.clone();
    let writing = settings.clone();
    Binding::controlled(
        Signal::derive_local(move || f64::from(reading.with(&read))),
        move |value: f64| writing.edit(|config| write(config, value)),
    )
}

/// Whether the settings are showing, as a modal.
///
/// A modal, because that is what settings are: something opened, changed, and closed again. The
/// tab remains for anybody who wants it beside the file whose behaviour they are changing, and
/// `BufferKind::Settings` renders the same page.
#[derive(Clone)]
pub struct ConfigModalState {
    open: zgui::reactive::RwSignal<bool, zgui::reactive::LocalStorage>,
}

impl Default for ConfigModalState {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigModalState {
    /// Closed.
    #[must_use]
    pub fn new() -> Self {
        Self {
            open: zgui::reactive::RwSignal::new_local(false),
        }
    }

    /// Whether it is up. Tracked.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open.get()
    }

    /// Whether it is, without subscribing.
    #[must_use]
    pub fn is_open_untracked(&self) -> bool {
        self.open.get_untracked()
    }

    /// Shows it.
    pub fn open(&self) {
        if !self.open.get_untracked() {
            self.open.set(true);
        }
    }

    /// Puts it away.
    ///
    /// Nothing here hands the keyboard back. The panel takes it while it is up, because `Escape`
    /// has to reach it, and the region underneath takes it back when the claim goes.
    pub fn close(&self) {
        if self.open.get_untracked() {
            self.open.set(false);
        }
    }
}

/// Puts the settings modal where every component can find it.
pub fn provide(state: ConfigModalState) {
    zgui::reactive::provide_local_context(state);
}

/// It, from inside a component.
#[must_use]
pub fn use_config_modal() -> Option<ConfigModalState> {
    zgui::reactive::use_local_context::<ConfigModalState>()
}

/// How a scheme is written in the settings file.
const fn scheme_name(scheme: zdt_core::config::Scheme) -> &'static str {
    match scheme {
        zdt_core::config::Scheme::Light => "light",
        zdt_core::config::Scheme::Dark => "dark",
        zdt_core::config::Scheme::System => "system",
    }
}

/// The reverse, defaulting to dark for anything unrecognised.
fn scheme_of(name: &str) -> zdt_core::config::Scheme {
    match name {
        "light" => zdt_core::config::Scheme::Light,
        "system" => zdt_core::config::Scheme::System,
        _ => zdt_core::config::Scheme::Dark,
    }
}

/// How a line-numbering choice is written in the settings file.
const fn line_numbers_name(numbers: zdt_core::config::LineNumbers) -> &'static str {
    match numbers {
        zdt_core::config::LineNumbers::Absolute => "absolute",
        zdt_core::config::LineNumbers::Relative => "relative",
        zdt_core::config::LineNumbers::None => "none",
    }
}

/// The reverse.
fn line_numbers_of(name: &str) -> zdt_core::config::LineNumbers {
    match name {
        "absolute" => zdt_core::config::LineNumbers::Absolute,
        "none" => zdt_core::config::LineNumbers::None,
        _ => zdt_core::config::LineNumbers::Relative,
    }
}

/// How a way of drawing tool calls is written in the settings file.
const fn activity_name(activity: zdt_core::config::Activity) -> &'static str {
    match activity {
        zdt_core::config::Activity::Grouped => "grouped",
        zdt_core::config::Activity::Verbose => "verbose",
    }
}

/// The reverse, defaulting to grouped for anything unrecognised.
fn activity_of(name: &str) -> zdt_core::config::Activity {
    match name {
        "verbose" => zdt_core::config::Activity::Verbose,
        _ => zdt_core::config::Activity::Grouped,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        activity_name, activity_of, line_numbers_name, line_numbers_of, scheme_name, scheme_of,
    };
    use zdt_core::config::{Activity, LineNumbers, Scheme};

    #[test]
    fn every_scheme_survives_the_round_trip() {
        // The panel writes the name and the file reads it back, so a name that did not round trip
        // would be a setting that silently reverted the moment it was saved.
        for scheme in [Scheme::Light, Scheme::Dark, Scheme::System] {
            assert_eq!(scheme_of(scheme_name(scheme)), scheme);
        }
    }

    #[test]
    fn every_numbering_survives_it_too() {
        for numbers in [
            LineNumbers::Absolute,
            LineNumbers::Relative,
            LineNumbers::None,
        ] {
            assert_eq!(line_numbers_of(line_numbers_name(numbers)), numbers);
        }
    }

    #[test]
    fn every_way_of_drawing_tool_calls_survives_it_too() {
        for activity in [Activity::Grouped, Activity::Verbose] {
            assert_eq!(activity_of(activity_name(activity)), activity);
        }
    }

    #[test]
    fn the_names_are_the_ones_the_file_uses() {
        // Which is what makes the panel and a hand-written config.toml agree.
        assert_eq!(scheme_name(Scheme::System), "system");
        assert_eq!(line_numbers_name(LineNumbers::Relative), "relative");

        let written = toml::to_string(&zdt_core::Config::default()).expect("it writes");
        assert!(written.contains("scheme = \"dark\""), "{written}");
        assert!(written.contains("line_numbers = \"relative\""), "{written}");
        assert_eq!(activity_name(Activity::Grouped), "grouped");
        assert!(written.contains("activity = \"grouped\""), "{written}");
    }
}
