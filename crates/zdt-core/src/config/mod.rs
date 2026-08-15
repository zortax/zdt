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
    Config, Editor, Keys, LineNumbers, Lsp, Picker, Scheme, Server, Terminal, Tree, Ui,
};

/// Where the configuration directory is.
///
/// `$ZDT_CONFIG_DIR` when it is set, which is what a test and a second installation both need;
/// otherwise the platform's own, under `zdt`.
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

#[cfg(test)]
mod tests {
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
        // Somebody wrote it and meant something; carrying on with the defaults would hide that.
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
