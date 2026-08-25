//! What a picker is picking from.
//!
//! Two kinds, and the difference matters more than the list of names suggests.
//!
//! A **standing** source is a list that exists before anybody types: the files in the project,
//! the open buffers, the themes. It is gathered once when the picker opens and then only ranked,
//! which is why typing in it costs nothing.
//!
//! A **live** source is one where the query *is* the search, which is grep. Every keystroke starts
//! a new search and cancels the one before it. Nothing is ranked, and the order is the order the
//! files came back in.

use std::path::PathBuf;

/// Which files a source looks at.
///
/// The keymap says `hidden` and `ignored` separately, so this does too: they are the two things a
/// person actually wants to change, and folding them into one flag would make `<Leader>fF` a lie.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Reach {
    /// Whether to include names beginning with a dot.
    pub hidden: bool,
    /// Whether to include what git ignores.
    pub ignored: bool,
}

impl Reach {
    /// What a keymap row's arguments ask for.
    #[must_use]
    pub fn of(args: &zdt_vim::Args) -> Self {
        Self {
            hidden: args.flag("hidden"),
            ignored: args.flag("ignored"),
        }
    }
}

/// Which picker is open.
#[derive(Clone, PartialEq, Debug)]
pub enum Source {
    /// A list somebody else worked out, with a name to put at the top.
    ///
    /// What every language-server picker is: the references, the symbols in a file, the actions
    /// offered at the caret. Each is one request whose answer is a list, and the picker could
    /// gather none of them itself. So the picker filters the list and lets somebody choose, which
    /// is the job it was already good at.
    Given {
        /// What to call it.
        title: &'static str,
        /// What to choose from.
        rows: Vec<Row>,
        /// What to do with text that matched nothing, when this list takes such a thing.
        ///
        /// A sessionizer is the reason it exists: the configured directories are a convenience,
        /// and any path at all must still be openable by typing it.
        typed: Option<Typed>,
    },
    /// Everything in the project whose name matches what is typed.
    ///
    /// Live, like grep: the query *is* the request, because no server will list every symbol in a
    /// project and none should be asked to.
    WorkspaceSymbols,
    /// The files in the project.
    Files {
        /// Which files to walk.
        reach: Reach,
    },
    /// The lines in the project, searched as the query is typed.
    Grep {
        /// Which files to look in.
        reach: Reach,
        /// What to start the query as. The word under the caret, for `<Leader>fc`.
        start: String,
    },
    /// The open buffers.
    Buffers,
    /// The lines of the buffer being edited.
    Lines,
    /// The themes that can be switched to.
    Themes,
    /// Everything the keymap can do, by description.
    Commands,
    /// Every key that is bound, by what it does.
    Keymaps,
    /// The files opened this session that are not open now.
    Recent,
    /// What is in each register.
    Registers,
    /// Where each mark is.
    Marks,
    /// The files git is tracking.
    GitFiles,
}

impl Source {
    /// What the picker calls itself.
    #[must_use]
    pub fn title(&self) -> &'static str {
        match self {
            Self::Given { title, .. } => title,
            Self::WorkspaceSymbols => "Project symbols",
            Self::Files { reach } if reach.ignored => "All files",
            Self::Files { .. } => "Files",
            Self::Grep { reach, .. } if reach.ignored => "Search everything",
            Self::Grep { .. } => "Search",
            Self::Buffers => "Buffers",
            Self::Lines => "Lines",
            Self::Themes => "Themes",
            Self::Commands => "Commands",
            Self::Keymaps => "Keys",
            Self::Recent => "Recent",
            Self::Registers => "Registers",
            Self::Marks => "Marks",
            Self::GitFiles => "Git files",
        }
    }

    /// Whether the query is the search itself. A filter over a list otherwise.
    #[must_use]
    pub fn is_live(&self) -> bool {
        matches!(self, Self::Grep { .. } | Self::WorkspaceSymbols)
    }

    /// Whether its rows are files worth previewing.
    ///
    /// A given list previews when its rows are files, which is a question about the rows rather
    /// than about the source: references and symbols are places in files and want a preview, and
    /// a list of code actions is not and does not.
    #[must_use]
    pub fn previews(&self) -> bool {
        match self {
            Self::Given { rows, .. } => rows
                .iter()
                .any(|row| matches!(row.target, Target::File { .. })),
            Self::WorkspaceSymbols => true,
            _ => matches!(
                self,
                Self::Files { .. }
                    | Self::Grep { .. }
                    | Self::Buffers
                    | Self::Lines
                    | Self::Recent
                    | Self::GitFiles
            ),
        }
    }

    /// What this list does with text that matched nothing, when it does anything.
    #[must_use]
    pub fn typed(&self) -> Option<&Typed> {
        match self {
            Self::Given { typed, .. } => typed.as_ref(),
            _ => None,
        }
    }

    /// What the query starts out as.
    #[must_use]
    pub fn start(&self) -> String {
        match self {
            Self::Grep { start, .. } => start.clone(),
            _ => String::new(),
        }
    }

    /// The source a keymap action names, when it names one that is built.
    #[must_use]
    pub fn named(leaf: &str, args: &zdt_vim::Args) -> Option<Self> {
        Some(match leaf {
            "files" => Self::Files {
                reach: Reach::of(args),
            },
            // `<Leader>fc` is the same picker with the word under the caret already in it; the
            // caller fills that in, because it is the only place that can see the caret.
            "grep" => Self::Grep {
                reach: Reach::of(args),
                start: String::new(),
            },
            "buffers" => Self::Buffers,
            "lines" => Self::Lines,
            "themes" => Self::Themes,
            "commands" => Self::Commands,
            "keymaps" => Self::Keymaps,
            "oldfiles" => Self::Recent,
            "registers" => Self::Registers,
            "marks" => Self::Marks,
            "git_files" => Self::GitFiles,
            _ => return None,
        })
    }
}

/// What choosing a row does.
#[derive(Clone, PartialEq, Debug)]
pub enum Target {
    /// Opens a file, at a line when there is one.
    File {
        /// Which file.
        path: PathBuf,
        /// Which line, counting from one.
        line: Option<u64>,
        /// Which bytes of that line matched, for the preview to pick out.
        matched: Option<std::ops::Range<usize>>,
    },
    /// Shows a buffer that is already open.
    Buffer(crate::workspace::BufferId),
    /// Puts the caret on a line of the buffer being edited.
    Line(u64),
    /// Switches to a theme.
    Theme(String),
    /// Runs an action, by the name the keymap knows it as.
    Action(zdt_vim::Action),
    /// Runs whatever the row was built to run.
    ///
    /// For rows whose behaviour has no name. A code action carries a whole protocol value that
    /// has to be resolved and applied, and a keymap has no way to say that. The work is held as a
    /// shared closure, because the row is *written* where the answer arrived and *read* where the
    /// picker draws it.
    Run(Deed),
    /// Nothing. A row that is there to be read.
    Nothing,
}

/// Something a row does when it is chosen.
#[derive(Clone)]
pub struct Deed(std::rc::Rc<dyn Fn()>);

impl Deed {
    /// A deed that runs `work`.
    #[must_use]
    pub fn new(work: impl Fn() + 'static) -> Self {
        Self(std::rc::Rc::new(work))
    }

    /// Does it.
    pub fn run(&self) {
        (self.0)();
    }
}

/// What a picker does with what was typed, when nothing in the list matched it.
///
/// A `Deed` that is handed the query. Kept apart from `Deed` because the two are asked at
/// different moments: a deed belongs to a row, and this belongs to the list.
#[derive(Clone)]
pub struct Typed(std::rc::Rc<dyn Fn(&str)>);

impl Typed {
    /// A handler that runs `work` on whatever was typed.
    #[must_use]
    pub fn new(work: impl Fn(&str) + 'static) -> Self {
        Self(std::rc::Rc::new(work))
    }

    /// Does it.
    pub fn run(&self, query: &str) {
        (self.0)(query);
    }
}

/// The same rule as [`Deed`]: two are the same when they are the same closure.
impl PartialEq for Typed {
    fn eq(&self, other: &Self) -> bool {
        std::rc::Rc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for Typed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Typed")
    }
}

/// Two deeds are the same when they are the same closure, which is the only answer a function has.
///
/// The rows are compared to decide whether the list changed, so this has to exist; comparing by
/// pointer is exactly right, because a rebuilt list is a different list.
impl PartialEq for Deed {
    fn eq(&self, other: &Self) -> bool {
        std::rc::Rc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for Deed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Deed")
    }
}

/// Which glyph a kind of symbol gets, and what colour it is drawn in.
///
/// The same four groups the completion popup uses, for the same reason: what somebody reads off a
/// glyph at twelve pixels is "is this a function, a type, a value or a word", and twenty-five
/// distinct glyphs would be twenty-five glyphs nobody can tell apart.
#[must_use]
pub fn symbol_mark(kind: lsp_types::SymbolKind) -> (&'static str, &'static str) {
    use lsp_types::SymbolKind as Kind;

    match kind {
        Kind::FUNCTION | Kind::METHOD => ("\u{f0295}", "zdt-completion-function"),
        Kind::CONSTRUCTOR => ("\u{f0674}", "zdt-completion-function"),
        Kind::CLASS | Kind::STRUCT => ("\u{f0233}", "zdt-completion-type"),
        Kind::INTERFACE => ("\u{f0e8}", "zdt-completion-type"),
        Kind::ENUM => ("\u{f0a5c}", "zdt-completion-type"),
        Kind::ENUM_MEMBER => ("\u{f0a5c}", "zdt-completion-value"),
        Kind::MODULE | Kind::NAMESPACE | Kind::PACKAGE => ("\u{f0487}", "zdt-completion-keyword"),
        Kind::VARIABLE => ("\u{f0b97}", "zdt-completion-value"),
        Kind::FIELD | Kind::PROPERTY => ("\u{f0ad1}", "zdt-completion-value"),
        Kind::CONSTANT => ("\u{f0ff2}", "zdt-completion-value"),
        Kind::FILE => ("\u{f0214}", "zdt-completion-text"),
        _ => ("\u{f0219}", "zdt-completion-text"),
    }
}

/// A list of places, as rows.
#[must_use]
pub fn location_rows(locations: &[lsp_types::Location], root: &std::path::Path) -> Vec<Row> {
    locations
        .iter()
        .filter_map(|location| {
            let path = zdt_lsp::convert::path_of(&location.uri)?;
            Some(Row::location(
                &path,
                u64::from(location.range.start.line) + 1,
                root,
            ))
        })
        .collect()
}

/// A project's symbols, as rows.
#[must_use]
pub fn symbol_rows(symbols: &[zdt_lsp::Symbol], root: &std::path::Path) -> Vec<Row> {
    symbols
        .iter()
        .filter_map(|symbol| {
            let path = zdt_lsp::convert::path_of(&symbol.uri)?;
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            let (glyph, tint) = symbol_mark(symbol.kind);
            // The name first, because that is what is being searched for; where it is goes in the
            // dim text after it, because that is what tells two of the same name apart.
            let label = match symbol.container.as_deref().filter(|it| !it.is_empty()) {
                Some(container) => format!("{container}::{}", symbol.name),
                None => symbol.name.clone(),
            };
            Some(
                Row {
                    label,
                    detail: format!("{relative}:{}", symbol.range.start.line + 1),
                    matched: Vec::new(),
                    glyph: Some(glyph),
                    tint: Some(tint),
                    icon: None,
                    target: Target::File {
                        path,
                        line: Some(u64::from(symbol.range.start.line) + 1),
                        matched: None,
                    },
                }
                .to_owned(),
            )
        })
        .collect()
}

/// What the preview shows for one row.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Preview {
    /// Which file.
    pub path: PathBuf,
    /// Which line to put in the middle, counting from one.
    pub line: Option<u64>,
    /// Which bytes of that line the search matched.
    pub matched: Option<std::ops::Range<usize>>,
}

/// One row of a picker.
#[derive(Clone, PartialEq, Debug)]
pub struct Row {
    /// What it says.
    pub label: String,
    /// What it says in the dimmer text after the label: a line's text, a key's binding.
    pub detail: String,
    /// Which bytes of the label the query landed on, for drawing.
    pub matched: Vec<u32>,
    /// A nerd-font glyph, when the row stands for a file.
    pub glyph: Option<&'static str>,
    /// Which colour the glyph takes.
    pub tint: Option<&'static str>,
    /// A filled vector mark, when the row stands for a provider. Drawn before the label in the
    /// glyph's place.
    pub icon: Option<&'static str>,
    /// What choosing it does.
    pub target: Target,
}

impl Row {
    /// A row with nothing but a label.
    #[must_use]
    pub fn plain(label: impl Into<String>, target: Target) -> Self {
        Self {
            label: label.into(),
            detail: String::new(),
            matched: Vec::new(),
            glyph: None,
            tint: None,
            icon: None,
            target,
        }
    }

    /// A row standing for a file, with the glyph its extension earns.
    #[must_use]
    pub fn file(relative: impl Into<String>, root: &std::path::Path, line: Option<u64>) -> Self {
        let relative = relative.into();
        let kind = zdt_core::language::of(std::path::Path::new(&relative));
        Self {
            label: relative.clone(),
            detail: String::new(),
            matched: Vec::new(),
            glyph: Some(kind.glyph),
            tint: Some(kind.tint),
            icon: None,
            target: Target::File {
                path: root.join(&relative),
                line,
                matched: None,
            },
        }
    }

    /// The same row, with `matched` filled in.
    #[must_use]
    pub fn with_matched(mut self, matched: Vec<u32>) -> Self {
        self.matched = matched;
        self
    }

    /// The same row, with a detail.
    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }

    /// The same row, saying which bytes of its line the search matched.
    #[must_use]
    pub fn with_match(mut self, matched: std::ops::Range<usize>) -> Self {
        if let Target::File { matched: held, .. } = &mut self.target
            && !matched.is_empty()
        {
            *held = Some(matched);
        }
        self
    }

    /// A row standing for a place in a file, named the way a person names one.
    ///
    /// The path relative to the project, and the line after it. An error message and a grep hit
    /// both look like that, so a list of references reads the same way as everything else the
    /// picker shows.
    #[must_use]
    pub fn location(path: &std::path::Path, line: u64, root: &std::path::Path) -> Self {
        let relative = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        let kind = zdt_core::language::of(path);
        Self {
            label: format!("{relative}:{line}"),
            detail: String::new(),
            matched: Vec::new(),
            glyph: Some(kind.glyph),
            tint: Some(kind.tint),
            icon: None,
            target: Target::File {
                path: path.to_path_buf(),
                line: Some(line),
                matched: None,
            },
        }
    }

    /// The same row, with a glyph and a tint of its own.
    #[must_use]
    pub fn with_glyph(mut self, glyph: &'static str, tint: &'static str) -> Self {
        self.glyph = Some(glyph);
        self.tint = Some(tint);
        self
    }

    /// The same row, wearing a filled vector mark.
    #[must_use]
    pub fn with_icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }

    /// The file this row previews, the line to scroll to, and what to pick out on it.
    #[must_use]
    pub fn preview(&self) -> Option<Preview> {
        match &self.target {
            Target::File {
                path,
                line,
                matched,
            } => Some(Preview {
                path: path.clone(),
                line: *line,
                matched: matched.clone(),
            }),
            _ => None,
        }
    }
}
