//! The settings, as the panel changes them.
//!
//! The panel itself is bindings over [`zdt::settings::Settings`], so what is asserted here is that
//! layer: that a change is live at once, that writing it out writes only what disagrees with the
//! defaults, and that the editor's own write does not come back around through the file watcher as
//! somebody else's change.

use zdt::settings::Settings;
use zdt_core::config::Paths;
use zgui_testkit_view::Window;

/// A configuration directory that removes itself.
struct Temp(std::path::PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "zdt-settings-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a directory");
        Self(root)
    }

    fn paths(&self) -> Paths {
        Paths::at(&self.0)
    }

    fn config(&self) -> String {
        std::fs::read_to_string(self.paths().config()).unwrap_or_default()
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs `body` inside a reactive scope, which the settings need to hold a signal.
fn in_scope<R>(body: impl FnOnce() -> R) -> R {
    let window = Window::open();
    window.scope.with(body)
}

#[test]
fn a_change_is_live_before_it_is_written() {
    // That is the whole point of the panel. The theme repaints and the tree re-filters as the
    // control moves, because everything reads the one signal.
    in_scope(|| {
        let settings = Settings::new(zdt_core::Config::default(), None);
        assert_eq!(settings.with(|config| config.editor.tab_size), 4);

        settings.edit(|config| config.editor.tab_size = 2);
        assert_eq!(
            settings.with(|config| config.editor.tab_size),
            2,
            "the running editor already has it"
        );
    });
}

#[test]
fn writing_writes_only_what_disagrees_with_the_defaults() {
    let temp = Temp::new("diff");
    in_scope(|| {
        let settings = Settings::new(zdt_core::Config::default(), Some(temp.paths()));
        settings.update(|config| config.editor.scrolloff = 9);
        settings.persist().expect("it writes");
    });

    let written = temp.config();
    assert!(written.contains("scrolloff = 9"), "{written}");
    assert!(
        !written.contains("tab_size"),
        "everything else is left to the defaults:\n{written}"
    );
}

#[test]
fn what_is_written_reads_back_as_what_was_meant() {
    let temp = Temp::new("roundtrip");
    in_scope(|| {
        let settings = Settings::new(zdt_core::Config::default(), Some(temp.paths()));
        settings.update(|config| {
            config.ui.theme = "gruvbox".to_owned();
            config.editor.tab_size = 2;
            config.editor.completion_doc_delay = 400;
        });
        settings.persist().expect("it writes");
    });

    let read = zdt_core::config::load(&temp.paths().config()).expect("it reads");
    assert_eq!(read.ui.theme, "gruvbox");
    assert_eq!(read.editor.tab_size, 2);
    assert_eq!(read.editor.completion_doc_delay, 400);
    assert_eq!(
        read.editor.scrolloff, 3,
        "and everything untouched is still the default"
    );
}

#[test]
fn the_editors_own_write_does_not_come_back_around() {
    // The defect this prevents: dragging a slider writes the file, the watcher reports the write,
    // and the reload applies what is already applied and announces "configuration reloaded". Once
    // per pixel the slider moved.
    let temp = Temp::new("stamp");
    in_scope(|| {
        let settings = Settings::new(zdt_core::Config::default(), Some(temp.paths()));
        settings.update(|config| config.editor.scrolloff = 9);
        settings.persist().expect("it writes");

        let written = temp.config();
        assert!(
            settings.wrote(Some(&written)),
            "the editor recognises what it just wrote"
        );
    });
}

#[test]
fn somebody_elses_write_is_not_mistaken_for_the_editors() {
    let temp = Temp::new("other");
    in_scope(|| {
        let settings = Settings::new(zdt_core::Config::default(), Some(temp.paths()));
        settings.update(|config| config.editor.scrolloff = 9);
        settings.persist().expect("it writes");

        assert!(
            !settings.wrote(Some("[editor]\nscrolloff = 2\n")),
            "a different file is a different change"
        );
        assert!(
            !settings.wrote(None),
            "and no file at all is not one either"
        );
    });
}

#[test]
fn the_stamp_is_taken_rather_than_kept() {
    // Otherwise the *second* time somebody else writes exactly what the editor once wrote, it
    // would be ignored, and what they changed would silently stay unapplied.
    let temp = Temp::new("stamp-once");
    in_scope(|| {
        let settings = Settings::new(zdt_core::Config::default(), Some(temp.paths()));
        settings.update(|config| config.editor.scrolloff = 9);
        settings.persist().expect("it writes");

        let written = temp.config();
        assert!(settings.wrote(Some(&written)), "the first time it is ours");
        assert!(
            !settings.wrote(Some(&written)),
            "the second time it is somebody else writing the same thing"
        );
    });
}

#[test]
fn settings_with_nowhere_to_write_do_not_complain() {
    // Every test that builds settings without a configuration directory, and every run with
    // `$HOME` unset.
    in_scope(|| {
        let settings = Settings::new(zdt_core::Config::default(), None);
        settings.edit(|config| config.editor.tab_size = 2);
        settings.persist().expect("it does nothing, successfully");
        assert_eq!(settings.with(|config| config.editor.tab_size), 2);
    });
}

#[test]
fn the_defaults_write_an_empty_file() {
    // So that somebody who opens the panel, looks, and changes nothing does not end up with two
    // hundred lines of configuration they did not ask for.
    let temp = Temp::new("empty");
    in_scope(|| {
        let settings = Settings::new(zdt_core::Config::default(), Some(temp.paths()));
        settings.persist().expect("it writes");
    });
    assert!(temp.config().trim().is_empty(), "{}", temp.config());
}
