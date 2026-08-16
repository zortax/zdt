//! Reading the configuration again after it changed.
//!
//! The watch itself is [`zdt_view::watch`]. What it reports is *that* something changed. This is
//! the other half: reading the files again and putting the results where the interface reads them.
//!
//! Every part of the configuration can change while the editor is running, which is what makes it
//! worth changing. A theme is judged by looking at it, and a keymap by using it.

use std::path::PathBuf;

use zdt_core::config::Paths;

/// Watches the configuration directory and calls `changed` on the interface thread.
///
/// The returned handle keeps the watch alive. Dropping it stops the watching.
#[must_use]
pub fn watch(paths: &Paths, changed: impl Fn() + 'static) -> Option<zdt_view::Watcher> {
    zdt_view::watch(&paths.root, changed)
}

/// Everything a change to the configuration directory can bring.
#[derive(Debug, Default)]
pub struct Reloaded {
    /// The settings, when they read.
    pub config: Option<zdt_core::Config>,
    /// Exactly what was in the settings file, for telling this editor's own write apart from
    /// somebody else's.
    pub config_text: Option<String>,
    /// The keymap, when there is one.
    pub keymap: Option<String>,
    /// The user's own style sheet, when there is one.
    pub user_css: Option<String>,
    /// What went wrong, if anything did.
    pub problems: Vec<String>,
}

/// Reads everything in `paths` again.
///
/// Blocking, and called from a worker: four small files, but a configuration directory can be on
/// a network share and the interface thread must not wait on one.
#[must_use]
pub fn read(paths: &Paths) -> Reloaded {
    let mut reloaded = Reloaded::default();

    match zdt_core::config::load(&paths.config()) {
        Ok(config) => reloaded.config = Some(config),
        Err(error) => reloaded.problems.push(error.to_string()),
    }
    reloaded.config_text = zdt_core::config::read_optional(&paths.config());
    reloaded.keymap = zdt_core::config::read_optional(&paths.keymap());
    reloaded.user_css = zdt_core::config::read_optional(&paths.user_css());

    reloaded
}

/// The themes a person has written, by name.
///
/// Both files of each, so a theme with only a dark one is still a theme.
#[must_use]
pub fn user_themes(paths: &Paths) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(paths.themes()) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let stem = name.strip_suffix(".css")?;
            let base = stem
                .strip_suffix("-light")
                .or_else(|| stem.strip_suffix("-dark"))?;
            Some(base.to_owned())
        })
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// Every path the editor would like to be told about.
#[must_use]
pub fn watched(paths: &Paths) -> Vec<PathBuf> {
    paths.watched()
}

#[cfg(test)]
mod tests {
    use zdt_core::config::Paths;

    use super::{read, user_themes};

    fn temporary() -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "zdt-reload-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("the directory is made");
        directory
    }

    #[test]
    fn an_empty_directory_reads_as_the_defaults_and_nothing_else() {
        let directory = temporary();
        let paths = Paths::at(&directory);
        let reloaded = read(&paths);
        assert!(reloaded.problems.is_empty());
        assert_eq!(reloaded.config, Some(zdt_core::Config::default()));
        assert!(reloaded.keymap.is_none());
        assert!(reloaded.user_css.is_none());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn everything_a_person_wrote_comes_back() {
        let directory = temporary();
        let paths = Paths::at(&directory);
        std::fs::write(paths.config(), "[editor]\nscrolloff = 9\n").expect("it writes");
        std::fs::write(paths.keymap(), "[[map]]\nkeys = \"gq\"\naction = \"a.b\"\n")
            .expect("it writes");
        std::fs::write(paths.user_css(), ":root { --zdt-danger: red; }").expect("it writes");

        let reloaded = read(&paths);
        assert_eq!(reloaded.config.expect("it read").editor.scrolloff, 9);
        assert!(reloaded.keymap.expect("it read").contains("gq"));
        assert!(reloaded.user_css.expect("it read").contains("--zdt-danger"));
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_broken_settings_file_is_reported_and_the_rest_is_still_read() {
        // A typo in the settings must not cost somebody their keymap.
        let directory = temporary();
        let paths = Paths::at(&directory);
        std::fs::write(paths.config(), "[editor]\nnonsense = 1\n").expect("it writes");
        std::fs::write(paths.keymap(), "[[map]]\nkeys = \"gq\"\naction = \"a.b\"\n")
            .expect("it writes");

        let reloaded = read(&paths);
        assert_eq!(reloaded.problems.len(), 1);
        assert!(reloaded.config.is_none());
        assert!(reloaded.keymap.is_some());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_theme_is_found_by_either_of_its_files() {
        let directory = temporary();
        let paths = Paths::at(&directory);
        std::fs::create_dir_all(paths.themes()).expect("the directory is made");
        std::fs::write(paths.themes().join("mine-dark.css"), "").expect("it writes");
        std::fs::write(paths.themes().join("ours-light.css"), "").expect("it writes");
        std::fs::write(paths.themes().join("ours-dark.css"), "").expect("it writes");
        std::fs::write(paths.themes().join("notes.txt"), "").expect("it writes");

        assert_eq!(user_themes(&paths), vec!["mine", "ours"]);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_directory_with_no_themes_in_it_has_none() {
        let directory = temporary();
        assert!(user_themes(&Paths::at(&directory)).is_empty());
        let _ = std::fs::remove_dir_all(&directory);
    }
}
