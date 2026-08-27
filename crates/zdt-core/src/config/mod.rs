//! Where the configuration lives, and how it is read.
//!
//! One directory holds everything a person can change: the settings, the keymap, their own themes,
//! a style sheet of their own, and the grammars and queries for languages the editor does not ship
//! with.
//!
//! ```text
//! ~/.config/zdt/
//!     config.toml          the settings
//!     keymap.toml          read after the shipped one, so it overrides
//!     user.css             installed last, so it overrides everything
//!     themes/<name>-light.css
//!     themes/<name>-dark.css
//!     grammars/<lang>.so
//!     queries/<lang>/highlights.scm
//! ```
//!
//! Nothing in it has to exist. A missing file is the same as an empty one, so a first run needs no
//! setting up at all.

mod schema;

use std::path::{Path, PathBuf};

pub use crate::config::schema::{
    Activity, Config, Editor, Instance, Keys, Leap, LineNumbers, Lsp, Picker, Scheme, Server,
    Sessions, Terminal, Tree, Ui,
};

/// `text` as a path, with a leading `~` replaced by the home directory.
///
/// What a configured path is read through. A file somebody writes by hand says `~/Projects`,
/// because that is what every other configuration file they have says.
///
/// Only a leading `~` on its own or before a separator. A directory really called `~stuff` is a
/// directory really called `~stuff`, and no shell expands that either.
#[must_use]
pub fn expand_home(text: &str) -> PathBuf {
    let Some(rest) = text.strip_prefix('~') else {
        return PathBuf::from(text);
    };
    if !rest.is_empty() && !rest.starts_with('/') && !rest.starts_with('\\') {
        return PathBuf::from(text);
    }
    let Some(home) = dirs::home_dir() else {
        return PathBuf::from(text);
    };
    match rest.strip_prefix(['/', '\\']) {
        Some(rest) => home.join(rest),
        None => home,
    }
}

/// `path` written with the home directory as `~`.
///
/// The other way round from [`expand_home`], and for the same reason: a person reads `~/Projects`
/// faster than they read their own home directory spelled out, and a column of paths that all
/// begin with the same twenty characters tells them apart nowhere near the start.
#[must_use]
pub fn shorten_home(path: &Path) -> String {
    match dirs::home_dir().and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// Where the configuration directory is.
///
/// `$ZDT_CONFIG_DIR` when it is set. A test and a second installation both need that. Otherwise
/// the platform's own directory, under `zdt`.
#[must_use]
pub fn directory() -> Option<PathBuf> {
    if let Some(named) = std::env::var_os("ZDT_CONFIG_DIR") {
        return Some(PathBuf::from(named));
    }
    dirs::config_dir().map(|base| base.join("zdt"))
}

/// The paths inside one configuration directory.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Paths {
    /// The directory itself.
    pub root: PathBuf,
}

impl Paths {
    /// The paths under `root`.
    #[must_use]
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The paths under the platform's configuration directory, when there is one.
    #[must_use]
    pub fn discover() -> Option<Self> {
        directory().map(Self::at)
    }

    /// The settings.
    #[must_use]
    pub fn config(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    /// The keymap read after the shipped one.
    #[must_use]
    pub fn keymap(&self) -> PathBuf {
        self.root.join("keymap.toml")
    }

    /// The file tree's own keys, read after the shipped ones.
    #[must_use]
    pub fn tree_keymap(&self) -> PathBuf {
        self.root.join("keymap-tree.toml")
    }

    /// The style sheet installed after everything else.
    #[must_use]
    pub fn user_css(&self) -> PathBuf {
        self.root.join("user.css")
    }

    /// Where a person's own themes are.
    #[must_use]
    pub fn themes(&self) -> PathBuf {
        self.root.join("themes")
    }

    /// Where grammars the editor does not ship with are.
    #[must_use]
    pub fn grammars(&self) -> PathBuf {
        self.root.join("grammars")
    }

    /// Where their queries are.
    #[must_use]
    pub fn queries(&self) -> PathBuf {
        self.root.join("queries")
    }

    /// Every file a change to which the editor should notice.
    #[must_use]
    pub fn watched(&self) -> Vec<PathBuf> {
        vec![
            self.config(),
            self.keymap(),
            self.tree_keymap(),
            self.user_css(),
            self.themes(),
        ]
    }
}

/// What went wrong reading the configuration.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The file could not be read.
    #[error("{path}: {source}")]
    Io {
        /// Which file.
        path: PathBuf,
        /// What the system said.
        #[source]
        source: std::io::Error,
    },
    /// The file is not the shape a configuration is.
    #[error("{path}: {source}")]
    Malformed {
        /// Which file.
        path: PathBuf,
        /// What was wrong with it.
        #[source]
        source: toml::de::Error,
    },
}

/// Reads the settings at `path`.
///
/// A file that is not there is every default, because a first run has nothing to configure yet. A
/// file that is there and wrong is an error, because somebody wrote it and meant something.
pub fn load(path: &Path) -> Result<Config, ConfigError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Config::default());
        }
        Err(source) => {
            return Err(ConfigError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };

    toml::from_str(&text).map_err(|source| ConfigError::Malformed {
        path: path.to_path_buf(),
        source,
    })
}

/// Reads a file that is allowed not to exist.
///
/// The keymap and the user's style sheet are both like this: absent is not empty in principle, but
/// it is in practice, and neither needs a first run to make one.
#[must_use]
pub fn read_optional(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// Writes a starting `config.toml` at `path`, if there is not one already.
///
/// Every default, written out with its comments, so that a person changing something can see what
/// else there is to change. Answers whether it wrote one.
pub fn write_default(path: &Path) -> Result<bool, ConfigError> {
    if path.exists() {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ConfigError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let body = toml::to_string_pretty(&Config::default()).unwrap_or_default();
    let text = format!(
        "# zdt's settings. Everything here is a default written out: delete what you agree with.\n\
         #\n\
         # The keymap is `keymap.toml` beside this, read after the one the editor ships with, so a\n\
         # row in it replaces the shipped row for the same keys. `action = false` removes one.\n\
         #\n\
         # A theme is two files in `themes/`: `<name>-light.css` and `<name>-dark.css`, each a\n\
         # block of custom-property declarations. `user.css` is installed after everything else.\n\
         \n{body}"
    );

    std::fs::write(path, text).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(true)
}

/// The settings as a file: every field that disagrees with the default, and nothing else.
///
/// What the settings panel writes. Serialising the whole `Config` would turn a three-line file
/// somebody wrote by hand into two hundred lines they did not. It would also freeze today's
/// defaults into their file, so a later change to one would never reach them.
///
/// # How far down it looks
///
/// One level. A section is compared key by key. Anything under a key, such as a table or an array,
/// is compared whole. This is a correctness rule. `lsp.servers` is a map with `#[serde(default)]`
/// on it, so a file that names *some* servers is a file that names *only* those. Written key by
/// key, adding one server would silently delete the four that ship.
#[must_use]
pub fn write_diff(config: &Config) -> String {
    let Ok(toml::Value::Table(current)) = toml::Value::try_from(config) else {
        return String::new();
    };
    let Ok(toml::Value::Table(default)) = toml::Value::try_from(Config::default()) else {
        return String::new();
    };

    let mut out = toml::Table::new();
    for (section, value) in current {
        let base = default.get(&section);
        match (&value, base) {
            (toml::Value::Table(mine), Some(toml::Value::Table(theirs))) => {
                let differs: toml::Table = mine
                    .iter()
                    .filter(|(key, value)| theirs.get(*key) != Some(*value))
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
                if !differs.is_empty() {
                    out.insert(section, toml::Value::Table(differs));
                }
            }
            // A key that is not a section, or a section the defaults have never heard of. Kept
            // whole.
            _ if base != Some(&value) => {
                out.insert(section, value);
            }
            _ => {}
        }
    }

    toml::to_string_pretty(&out).unwrap_or_default()
}

/// Writes `text` to `path` without ever leaving a half-written file there.
///
/// A temporary beside it, then a rename. The rename is atomic on every filesystem the editor runs
/// on. The watcher was written for this. It watches the directory, because an atomic save replaces
/// a file instead of writing into it.
///
/// # Errors
///
/// When the directory cannot be made, or either step fails.
pub fn write_atomically(path: &Path, text: &str) -> Result<(), ConfigError> {
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |source| ConfigError::Io {
            path: path.clone(),
            source,
        }
    };

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io(parent))?;
    }
    // Beside the target, and not in the temporary directory. A rename across filesystems is a
    // copy.
    let temporary = path.with_extension("toml.writing");
    std::fs::write(&temporary, text).map_err(io(&temporary))?;
    std::fs::rename(&temporary, path).map_err(io(path))
}

#[cfg(test)]
mod diff_tests {
    use super::{Config, write_atomically, write_diff};

    #[test]
    fn the_defaults_write_as_nothing_at_all() {
        // Which is the whole point: a person who has changed nothing has an empty file, and picks
        // up every default the editor changes later.
        assert_eq!(write_diff(&Config::default()).trim(), "");
    }

    #[test]
    fn one_change_writes_one_key() {
        let mut config = Config::default();
        config.editor.scrolloff = 8;

        let text = write_diff(&config);
        assert!(text.contains("scrolloff = 8"), "{text}");
        assert!(
            !text.contains("tab_size"),
            "everything else is left to the defaults:\n{text}"
        );
        assert!(!text.contains("[ui]"), "and so is every other section");
    }

    #[test]
    fn what_is_written_reads_back_as_what_was_meant() {
        let mut config = Config::default();
        config.ui.theme = "gruvbox".to_owned();
        config.editor.tab_size = 2;
        config.tree.width = 320;

        let read: Config = toml::from_str(&write_diff(&config)).expect("it reads");
        assert_eq!(read, config);
    }

    #[test]
    fn the_servers_are_written_whole_or_not_at_all() {
        // The defect this prevents is the worst kind. `lsp.servers` has `#[serde(default)]` on
        // it, so a file that names *some* servers names *only* those. Written key by key, adding
        // one server would silently delete the four that ship. The symptom appears a week later
        // as rust-analyzer quietly not starting.
        let mut config = Config::default();
        config.lsp.servers.insert(
            "zls".to_owned(),
            crate::config::schema::Server {
                command: "zls".to_owned(),
                filetypes: vec!["zig".to_owned()],
                ..Default::default()
            },
        );

        let text = write_diff(&config);
        assert!(text.contains("zls"), "the new one is there:\n{text}");
        assert!(
            text.contains("rust-analyzer"),
            "and so are the ones that ship:\n{text}"
        );

        let read: Config = toml::from_str(&text).expect("it reads");
        assert_eq!(read.lsp.servers.len(), config.lsp.servers.len());
        assert_eq!(read, config);
    }

    #[test]
    fn a_section_with_nothing_changed_in_it_is_not_written() {
        let mut config = Config::default();
        config.ui.font_size = 13.0;

        let text = write_diff(&config);
        assert!(text.contains("[ui]"));
        for absent in ["[editor]", "[terminal]", "[picker]", "[tree]", "[lsp]"] {
            assert!(!text.contains(absent), "{absent} is in:\n{text}");
        }
    }

    #[test]
    fn writing_never_leaves_half_a_file_behind() {
        // This asserts the arrangement that makes a race impossible. The write goes to a
        // temporary, and the rename publishes it.
        let directory = std::env::temp_dir().join(format!(
            "zdt-write-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        let path = directory.join("nested").join("config.toml");

        write_atomically(&path, "[editor]\nscrolloff = 4\n").expect("it writes");
        assert_eq!(
            std::fs::read_to_string(&path).expect("it is there"),
            "[editor]\nscrolloff = 4\n"
        );

        // Writing again replaces the file, with no leftovers beside it.
        write_atomically(&path, "[editor]\nscrolloff = 9\n").expect("it writes again");
        assert!(std::fs::read_to_string(&path).unwrap().contains("9"));
        assert!(
            !path.with_extension("toml.writing").exists(),
            "the temporary is renamed away, and none is left behind"
        );

        let _ = std::fs::remove_dir_all(&directory);
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_leading_tilde_becomes_the_home_directory() {
        let Some(home) = dirs::home_dir() else {
            return;
        };
        assert_eq!(super::expand_home("~/Projects"), home.join("Projects"));
        assert_eq!(super::expand_home("~"), home);
    }

    #[test]
    fn a_home_path_is_written_back_with_a_tilde() {
        let Some(home) = dirs::home_dir() else {
            return;
        };
        assert_eq!(super::shorten_home(&home.join("Projects")), "~/Projects");
        assert_eq!(super::shorten_home(&home), "~");
        assert_eq!(super::shorten_home(std::path::Path::new("/etc")), "/etc");
    }

    #[test]
    fn a_tilde_that_is_part_of_a_name_is_left_alone() {
        // No shell expands `~stuff` either, and a directory really called that must be reachable.
        assert_eq!(
            super::expand_home("~stuff"),
            std::path::PathBuf::from("~stuff")
        );
        assert_eq!(
            super::expand_home("/tmp/~/x"),
            std::path::PathBuf::from("/tmp/~/x")
        );
    }
    use super::{Paths, load, write_default};

    fn temporary() -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "zdt-config-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("the directory is made");
        directory
    }

    #[test]
    fn a_missing_file_is_every_default() {
        // A first run has nothing to configure yet.
        let paths = Paths::at("/nowhere/at/all");
        let config = load(&paths.config()).expect("a missing file reads");
        assert_eq!(config.ui.theme, "oldworld");
    }

    #[test]
    fn a_file_that_is_wrong_says_so() {
        // Somebody wrote it and meant something. Carrying on with the defaults would hide that.
        let directory = temporary();
        let path = directory.join("config.toml");
        std::fs::write(&path, "[ui]\nthemee = \"nope\"\n").expect("it writes");
        assert!(load(&path).is_err());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_paths_are_where_they_are_expected() {
        let paths = Paths::at("/home/someone/.config/zdt");
        assert!(paths.config().ends_with("config.toml"));
        assert!(paths.keymap().ends_with("keymap.toml"));
        assert!(paths.tree_keymap().ends_with("keymap-tree.toml"));
        assert!(paths.user_css().ends_with("user.css"));
        assert!(paths.themes().ends_with("themes"));
        assert_eq!(paths.watched().len(), 5);
    }

    #[test]
    fn a_starting_file_is_written_once() {
        let directory = temporary();
        let path = directory.join("config.toml");

        assert!(write_default(&path).expect("it writes"));
        assert!(path.exists());
        // What it wrote reads back as the defaults it was written from.
        let config = load(&path).expect("it reads");
        assert_eq!(config, super::Config::default());

        // And it never overwrites what somebody has since changed.
        std::fs::write(&path, "[editor]\nscrolloff = 99\n").expect("it writes");
        assert!(!write_default(&path).expect("it does not write"));
        assert_eq!(load(&path).expect("it reads").editor.scrolloff, 99);

        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_directory_can_be_told_where_to_be() {
        // Which is what a test needs, and what a second installation needs.
        // SAFETY: single-threaded within this test, and the variable is put back.
        unsafe { std::env::set_var("ZDT_CONFIG_DIR", "/tmp/zdt-elsewhere") };
        assert_eq!(
            super::directory(),
            Some(std::path::PathBuf::from("/tmp/zdt-elsewhere"))
        );
        unsafe { std::env::remove_var("ZDT_CONFIG_DIR") };
    }
}
