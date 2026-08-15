//! What a `config.toml` says.
//!
//! Every field has a default, so a configuration file is a list of disagreements with the editor
//! rather than a description of it. A missing file is the same as an empty one.
//!
//! Unknown fields are refused. A misspelled setting that silently did nothing is the worst kind of
//! configuration bug, because the only symptom is the editor not doing what the file plainly says.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The whole file.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    /// How the interface looks.
    pub ui: Ui,
    /// How the editor behaves.
    pub editor: Editor,
    /// How terminals are started.
    pub terminal: Terminal,
    /// How the pickers search.
    pub picker: Picker,
    /// Which keys are the leaders.
    pub keys: Keys,
    /// The language servers, by the name they are known as.
    pub lsp: Lsp,
}

/// How the interface looks.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Ui {
    /// Which theme, by the name of its files.
    pub theme: String,
    /// Which surface to present on: `light`, `dark`, or `system` to follow the desktop.
    pub scheme: Scheme,
    /// The interface font, which is also the one devicons are drawn in.
    pub font: String,
    /// Its size, in pixels.
    pub font_size: f32,
    /// How long a part-typed sequence sits before which-key appears, in milliseconds.
    pub whichkey_delay: u64,
    /// Whether the window draws its own frame.
    ///
    /// Off puts the desktop's title bar back, for a desktop whose own decorations are wanted.
    pub client_side_decorations: bool,
}

impl Default for Ui {
    fn default() -> Self {
        Self {
            theme: "oldworld".to_owned(),
            scheme: Scheme::Dark,
            font: "Mononoki Nerd Font".to_owned(),
            font_size: 12.0,
            whichkey_delay: 300,
            client_side_decorations: true,
        }
    }
}

/// Which surface the interface presents on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Scheme {
    /// Light.
    Light,
    /// Dark.
    #[default]
    Dark,
    /// Whichever the desktop asked for.
    System,
}

/// How the editor behaves.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Editor {
    /// The font text is edited in.
    pub font: String,
    /// Its size, in pixels.
    pub font_size: f32,
    /// How the gutter numbers its lines.
    pub line_numbers: LineNumbers,
    /// How many lines to keep between the caret and the edge of the view.
    pub scrolloff: usize,
    /// How wide a tab is drawn.
    pub tab_size: u32,
    /// Whether tab inserts spaces.
    pub expand_tab: bool,
    /// Whether the view glides rather than jumping.
    pub smooth_scroll: bool,
    /// Whether the caret's line is tinted.
    pub cursorline: bool,
    /// How many editors one window keeps ready for buffers it is not showing.
    pub mounted_per_window: usize,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            font: "Mononoki Nerd Font".to_owned(),
            font_size: 14.0,
            line_numbers: LineNumbers::Relative,
            scrolloff: 3,
            tab_size: 4,
            expand_tab: true,
            smooth_scroll: true,
            cursorline: true,
            mounted_per_window: 8,
        }
    }
}

/// How the gutter numbers its lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LineNumbers {
    /// The line's own number.
    Absolute,
    /// Its distance from the caret, and the caret's own number on its line.
    #[default]
    Relative,
    /// None at all.
    None,
}

/// How terminals are started.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Terminal {
    /// What to run. Empty means whatever `$SHELL` says.
    pub shell: String,
    /// How wide a floating terminal is, as a fraction of the window.
    pub float_width: f32,
    /// How tall.
    pub float_height: f32,
    /// How many lines of scrollback to keep.
    pub scrollback: usize,
}

impl Default for Terminal {
    fn default() -> Self {
        Self {
            shell: String::new(),
            float_width: 0.85,
            float_height: 0.8,
            scrollback: 10_000,
        }
    }
}

/// How the pickers search.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Picker {
    /// Whether to show what is under the caret beside the list.
    pub preview: bool,
    /// How many rows to show at once.
    pub max_results: usize,
    /// How large a file may be before only its head is previewed, in bytes.
    pub preview_max_bytes: u64,
    /// Whether a search with no capitals is case-insensitive.
    pub smart_case: bool,
    /// Whether to look inside files git ignores.
    pub ignored: bool,
    /// Whether to look at files whose names begin with a dot.
    pub hidden: bool,
}

impl Default for Picker {
    fn default() -> Self {
        Self {
            preview: true,
            max_results: 200,
            preview_max_bytes: 2 * 1024 * 1024,
            smart_case: true,
            ignored: false,
            hidden: false,
        }
    }
}

/// Which keys are the leaders.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Keys {
    /// What `<Leader>` stands for, in the keymap's own notation.
    pub leader: String,
    /// What `<LocalLeader>` stands for.
    pub local_leader: String,
}

impl Default for Keys {
    fn default() -> Self {
        Self {
            leader: "<Space>".to_owned(),
            local_leader: ",".to_owned(),
        }
    }
}

/// The language servers.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Lsp {
    /// Whether to start any at all.
    pub enabled: bool,
    /// The servers, by the name they are known as.
    pub servers: BTreeMap<String, Server>,
}

// Written out rather than derived, because a derived `Default` would leave language servers off —
// and a whole file with no `[lsp]` table in it takes every field from here.
impl Default for Lsp {
    fn default() -> Self {
        Self {
            enabled: true,
            servers: BTreeMap::new(),
        }
    }
}

/// One language server.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Server {
    /// The program to run.
    pub command: String,
    /// What to give it.
    pub args: Vec<String>,
    /// Which file types it answers for, by the grammar's name.
    pub filetypes: Vec<String>,
    /// The files whose presence marks the top of a project it should index.
    pub root_markers: Vec<String>,
    /// What to send as `initializationOptions`.
    pub initialization_options: Option<toml::Value>,
    /// What to send as the workspace configuration.
    pub settings: Option<toml::Value>,
    /// What to add to its environment.
    pub env: BTreeMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::{Config, LineNumbers, Scheme};

    #[test]
    fn an_empty_file_is_every_default() {
        let empty: Config = toml::from_str("").expect("an empty file reads");
        assert_eq!(empty, Config::default());
        assert_eq!(empty.ui.theme, "oldworld");
        assert_eq!(empty.editor.line_numbers, LineNumbers::Relative);
        assert!(empty.lsp.enabled);
    }

    #[test]
    fn a_file_is_a_list_of_disagreements() {
        // One setting changed leaves every other one alone, which is what makes a configuration
        // file readable a year later.
        let config: Config = toml::from_str("[editor]\nscrolloff = 8\n").expect("it reads");
        assert_eq!(config.editor.scrolloff, 8);
        assert_eq!(config.editor.tab_size, 4, "the rest are untouched");
        assert_eq!(config.ui.theme, "oldworld");
    }

    #[test]
    fn a_misspelled_setting_is_refused() {
        // Silently doing nothing is the worst kind of configuration bug: the only symptom is the
        // editor not doing what the file plainly says.
        assert!(toml::from_str::<Config>("[editor]\nscrollof = 8\n").is_err());
        assert!(toml::from_str::<Config>("[edtior]\nscrolloff = 8\n").is_err());
    }

    #[test]
    fn a_scheme_is_written_in_words() {
        let config: Config = toml::from_str("[ui]\nscheme = \"system\"\n").expect("it reads");
        assert_eq!(config.ui.scheme, Scheme::System);
        assert!(toml::from_str::<Config>("[ui]\nscheme = \"sepia\"\n").is_err());
    }

    #[test]
    fn a_language_server_is_a_table_under_its_name() {
        let config: Config = toml::from_str(
            r#"
            [lsp.servers.rust-analyzer]
            command = "rust-analyzer"
            filetypes = ["rust"]
            root_markers = ["Cargo.toml"]
            settings = { "rust-analyzer" = { check = { command = "clippy" } } }
            "#,
        )
        .expect("it reads");

        let server = config
            .lsp
            .servers
            .get("rust-analyzer")
            .expect("it is under its name");
        assert_eq!(server.command, "rust-analyzer");
        assert_eq!(server.filetypes, ["rust"]);
        assert!(server.settings.is_some());
        assert!(server.args.is_empty(), "what is not said is empty");
    }

    #[test]
    fn what_is_written_reads_back() {
        // Which is what lets the editor write a configuration file out for somebody to edit.
        let config = Config::default();
        let text = toml::to_string_pretty(&config).expect("it writes");
        let read: Config = toml::from_str(&text).expect("it reads back");
        assert_eq!(read, config);
    }
}
