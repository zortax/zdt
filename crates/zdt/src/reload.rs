//! Noticing that the configuration changed.
//!
//! A watcher on the configuration directory, debounced, reporting on the interface thread. What it
//! reports is *that* something changed; what to do about it is [`apply`], which reads the files
//! again and puts the results where the interface reads them.
//!
//! Every part of the configuration can be changed while the editor is running, which is what makes
//! it worth changing: a theme is judged by looking at it, and a keymap by using it.

use std::path::PathBuf;
use std::time::Duration;

use zdt_core::config::Paths;
use zgui::task::spawn_local;
use zgui::tokio::spawn_receiver;

/// How long to wait after a change before reading, so that a save that writes several files —
/// which every editor's atomic save does — is read once rather than four times.
const SETTLE: Duration = Duration::from_millis(120);

/// Watches `paths` and calls `changed` on the interface thread whenever something under it moves.
///
/// The watcher runs on its own thread and is kept alive by the returned handle; dropping it stops
/// the watching. A directory that does not exist is not an error — a person who has never
/// configured anything has no directory, and may make one later.
#[must_use]
pub fn watch(paths: &Paths, changed: impl Fn() + 'static) -> Option<Watcher> {
    use notify::{RecursiveMode, Watcher as _};

    // A tokio channel rather than the standard one: the receiving end is awaited on the interface
    // thread, and `spawn_receiver` is what turns it into a call there. The sending end goes to the
    // watcher's own thread, which is why it has to be one that crosses threads.
    let (tx, rx) = tokio::sync::mpsc::channel::<()>(16);

    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        // Anything that is not a read: written, made, removed, renamed.
        let interesting = event
            .map(|event| !matches!(event.kind, notify::EventKind::Access(_)))
            .unwrap_or(false);
        if interesting {
            // Full is fine: a change is a change, and one that arrives while another is being
            // dealt with is the same reload.
            let _ = tx.try_send(());
        }
    })
    .ok()?;

    // The directory rather than the files: an atomic save replaces a file rather than writing it,
    // and a watch on the file itself would follow the one that was renamed away. A directory that
    // is not there is not an error — somebody who has never configured anything has none, and may
    // make one later; the editor notices at the next start.
    if watcher.watch(&paths.root, RecursiveMode::Recursive).is_err() {
        return None;
    }

    // One task takes what arrives and reports it, after a pause: an atomic save writes a temporary
    // and renames it, which is two events for one change. The report is shared rather than moved
    // because it is called once per change and the channel goes on delivering.
    let changed = std::rc::Rc::new(changed);
    let pump = spawn_receiver(rx, move |()| {
        if held().replace(true) {
            // Something is already waiting to report; this change joins it.
            return;
        }
        let changed = std::rc::Rc::clone(&changed);
        let task = spawn_local(async move {
            zgui::task::blocking(move || std::thread::sleep(SETTLE)).await;
            held().set(false);
            changed();
        });
        // The task outlives this call by design; the watch's own handle is what ends the
        // reporting, because dropping it drops the channel and ends the pump.
        std::mem::forget(task);
    });

    Some(Watcher {
        _watcher: watcher,
        _pump: pump,
    })
}

/// Whether a report is already waiting to be made.
///
/// A cell on the interface thread rather than a flag in the closure, because the closure is called
/// again while the pause is running and both calls have to see the same answer.
fn held() -> &'static std::thread::LocalKey<std::cell::Cell<bool>> {
    thread_local! {
        static WAITING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    &WAITING
}

/// Keeps a watch alive. Dropping it stops the watching.
pub struct Watcher {
    _watcher: notify::RecommendedWatcher,
    _pump: zgui::task::Task,
}

/// Everything a change to the configuration directory can bring.
#[derive(Debug, Default)]
pub struct Reloaded {
    /// The settings, when they read.
    pub config: Option<zdt_core::Config>,
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
