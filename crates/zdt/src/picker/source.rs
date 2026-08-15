//! What a picker is picking from.
//!
//! Two kinds, and the difference matters more than the list of names suggests.
//!
//! A **standing** source is a list that exists before anybody types: the files in the project, the
//! open buffers, the themes. It is gathered once when the picker opens and then only ranked, which
//! is why typing in it costs nothing.
//!
//! A **live** source is one where the query *is* the search: grep. Every keystroke starts a new
//! search and cancels the one before it, and there is no ranking at all — the order is the order
//! the files came back in.

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
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Source {
    /// The files in the project.
    Files {
        /// Which files to walk.
        reach: Reach,
    },
    /// The lines in the project, searched as the query is typed.
    Grep {
        /// Which files to look in.
        reach: Reach,
        /// What to start the query as — the word under the caret, for `<Leader>fc`.
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

    /// Whether the query is the search itself rather than a filter over a list.
    #[must_use]
    pub fn is_live(&self) -> bool {
        matches!(self, Self::Grep { .. })
    }

    /// Whether its rows are files worth previewing.
    #[must_use]
    pub fn previews(&self) -> bool {
        matches!(
            self,
            Self::Files { .. }
                | Self::Grep { .. }
                | Self::Buffers
                | Self::Lines
                | Self::Recent
                | Self::GitFiles
        )
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
    /// Nothing — a row that is there to be read.
    Nothing,
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
