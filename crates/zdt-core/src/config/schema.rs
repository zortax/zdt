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
    /// What the file tree shows.
    pub tree: Tree,
    /// How leaping behaves.
    pub leap: Leap,
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
    /// How heavy it is drawn: 100 to 900, where 400 is regular and 700 is bold.
    pub font_weight: u16,
    /// How long a part-typed sequence sits before which-key appears, in milliseconds.
    pub whichkey_delay: u64,
    /// Whether the window draws its own frame.
    ///
    /// Off puts the desktop's title bar back, for a desktop whose own decorations are wanted.
    pub client_side_decorations: bool,
    /// Whether anything the editor was not asked for is announced in the corner.
    ///
    /// Off leaves the status line saying what state things are in and nothing saying what has
    /// just happened, which is what somebody who finds announcements distracting wants.
    pub notifications: bool,
    /// How long an announcement stays, in milliseconds.
    ///
    /// Zero means until it is dismissed. Failures ignore this and always wait to be read.
    pub notification_timeout: u64,
}

impl Default for Ui {
    fn default() -> Self {
        Self {
            theme: "oldworld".to_owned(),
            scheme: Scheme::Dark,
            font: "Mononoki Nerd Font".to_owned(),
            font_size: 12.0,
            font_weight: 400,
            whichkey_delay: 300,
            client_side_decorations: true,
            notifications: true,
            notification_timeout: 4000,
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
    /// How heavy it is drawn: 100 to 900, where 400 is regular and 700 is bold.
    pub font_weight: u16,
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
    /// How far the view may move and still jump rather than glide, in lines.
    ///
    /// Zero animates every scroll, `j` at the bottom of the view included. Raise it if a
    /// line-at-a-time glide feels like the view lagging behind the keystroke.
    pub smooth_scroll_min_lines: f64,
    /// Whether the caret's line is tinted.
    pub cursorline: bool,
    /// How many editors one window keeps ready for buffers it is not showing.
    pub mounted_per_window: usize,
    /// Whether typing offers suggestions.
    pub completion: bool,
    /// How many characters of a word are typed before it does.
    ///
    /// One asks as soon as a word starts, which is what makes suggestions feel like they were
    /// already there. Raise it for a server whose answers are slow enough to be distracting.
    pub completion_min_chars: usize,
    /// Whether resting on a suggestion opens what the server says about it.
    pub completion_doc: bool,
    /// How long the caret rests on a suggestion first, in milliseconds.
    ///
    /// Zero opens it at once, which is right for somebody reading the list and wrong for somebody
    /// holding `<C-n>` through it.
    pub completion_doc_delay: u64,
    /// Whether the editor marks the other places the symbol under the caret is used.
    pub highlight_symbol: bool,
    /// How long the caret rests before it does, in milliseconds.
    pub highlight_symbol_delay: u64,
    /// Whether saving runs the language server's formatter first.
    pub format_on_save: bool,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            font: "Mononoki Nerd Font".to_owned(),
            font_size: 14.0,
            font_weight: 400,
            line_numbers: LineNumbers::Relative,
            scrolloff: 3,
            tab_size: 4,
            expand_tab: true,
            smooth_scroll: true,
            smooth_scroll_min_lines: 0.0,
            cursorline: true,
            mounted_per_window: 8,
            completion: true,
            completion_min_chars: 1,
            completion_doc: true,
            completion_doc_delay: 250,
            highlight_symbol: true,
            highlight_symbol_delay: 200,
            format_on_save: false,
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

/// What the file tree shows.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Tree {
    /// Whether it opens with the window.
    pub open: bool,
    /// How wide it is, in pixels.
    pub width: u32,
    /// Whether to show what begins with a dot.
    pub hidden: bool,
    /// Whether to show what git ignores.
    pub ignored: bool,
    /// Whether to move the tree's caret onto the file the editor shows.
    pub follow: bool,
}

impl Default for Tree {
    fn default() -> Self {
        Self {
            open: false,
            width: 260,
            hidden: false,
            ignored: false,
            follow: true,
        }
    }
}

/// How leaping behaves.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Leap {
    /// The keys labels are drawn from, in the order they are handed out.
    ///
    /// The earliest are the ones the fingers are already on, so the order matters as much as the
    /// letters do.
    pub alphabet: String,
}

impl Default for Leap {
    fn default() -> Self {
        Self {
            alphabet: "sfnjklhodweimbuyvrgtaqpcxz".to_owned(),
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
            servers: shipped_servers(),
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

/// The servers the editor knows about without being told.
///
/// A short list on purpose. These are the ones whose name and arguments are stable and whose root
/// markers are not a matter of opinion; anything else is a row somebody adds, which is two lines in
/// `config.toml`. A server named here that is not installed says so once and is not tried again,
/// which costs a line in the status line and nothing else.
fn shipped_servers() -> BTreeMap<String, Server> {
    let mut servers = BTreeMap::new();

    servers.insert(
        "rust-analyzer".to_owned(),
        Server {
            command: "rust-analyzer".to_owned(),
            filetypes: vec!["rust".to_owned()],
            root_markers: vec!["Cargo.toml".to_owned(), "rust-project.json".to_owned()],
            ..Server::default()
        },
    );
    servers.insert(
        "basedpyright".to_owned(),
        Server {
            command: "basedpyright-langserver".to_owned(),
            args: vec!["--stdio".to_owned()],
            filetypes: vec!["python".to_owned()],
            root_markers: vec![
                "pyproject.toml".to_owned(),
                "setup.py".to_owned(),
                "requirements.txt".to_owned(),
            ],
            ..Server::default()
        },
    );
    servers.insert(
        "gopls".to_owned(),
        Server {
            command: "gopls".to_owned(),
            filetypes: vec!["go".to_owned()],
            root_markers: vec!["go.work".to_owned(), "go.mod".to_owned()],
            ..Server::default()
        },
    );
    servers.insert(
        "lua-language-server".to_owned(),
        Server {
            command: "lua-language-server".to_owned(),
            filetypes: vec!["lua".to_owned()],
            root_markers: vec![".luarc.json".to_owned(), "stylua.toml".to_owned()],
            ..Server::default()
        },
    );

    servers
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
