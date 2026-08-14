//! The modes, and which of them a binding applies in.

/// What the editor is doing with the keys it is given.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Mode {
    /// Keys are commands. Where everything starts and where `<Esc>` goes back to.
    #[default]
    Normal,
    /// Keys are text.
    Insert,
    /// Keys are text, replacing what is under the caret rather than pushing it along.
    Replace,
    /// A selection that grows character by character.
    Visual,
    /// A selection of whole lines.
    VisualLine,
    /// A rectangle.
    VisualBlock,
    /// A selection that typing replaces, which is what a snippet's placeholder is.
    Select,
    /// An operator has been typed and is waiting for what to apply to.
    OperatorPending,
    /// The line at the bottom, taking an ex command or a search.
    Command,
    /// The keys belong to the program running in a terminal.
    Terminal,
}

impl Mode {
    /// How this is written in a status line.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Replace => "REPLACE",
            Self::Visual => "VISUAL",
            Self::VisualLine => "V-LINE",
            Self::VisualBlock => "V-BLOCK",
            Self::Select => "SELECT",
            Self::OperatorPending => "PENDING",
            Self::Command => "COMMAND",
            Self::Terminal => "TERMINAL",
        }
    }

    /// Which colour the status line's block takes, as a data attribute.
    #[must_use]
    pub const fn tone(self) -> &'static str {
        match self {
            Self::Normal | Self::OperatorPending => "normal",
            Self::Insert => "insert",
            Self::Replace => "replace",
            Self::Visual | Self::VisualLine | Self::VisualBlock | Self::Select => "visual",
            Self::Command => "command",
            Self::Terminal => "terminal",
        }
    }

    /// Whether this is one of the three visual modes.
    #[must_use]
    pub const fn is_visual(self) -> bool {
        matches!(self, Self::Visual | Self::VisualLine | Self::VisualBlock)
    }

    /// Whether keys typed in this mode are text.
    #[must_use]
    pub const fn is_inserting(self) -> bool {
        matches!(self, Self::Insert | Self::Replace)
    }
}

/// Which modes one binding applies in.
///
/// A set rather than one mode, because almost every binding applies in several and writing it out
/// once per mode is how a keymap gets out of step with itself.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct ModeSet(u16);

impl ModeSet {
    /// Nothing.
    pub const NONE: Self = Self(0);

    /// The set holding exactly `mode`.
    #[must_use]
    pub const fn of(mode: Mode) -> Self {
        Self(1 << mode as u16)
    }

    /// Whether `mode` is in it.
    #[must_use]
    pub const fn has(self, mode: Mode) -> bool {
        self.0 & Self::of(mode).0 != 0
    }

    /// The union.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether it holds nothing.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The modes a keymap's mode letter stands for.
    ///
    /// Vim's letters: `n` normal, `i` insert, `r` replace, `v` every visual mode, `x` the
    /// character and line ones a `<C-v>` block is not, `b` the block one, `s` select, `o`
    /// operator-pending, `c` the command line, `t` a terminal. `a` is all of them, for a binding
    /// like `<F7>` that has to work wherever the person is.
    #[must_use]
    pub fn from_letter(letter: &str) -> Option<Self> {
        Some(match letter {
            "n" => Self::of(Mode::Normal),
            "i" => Self::of(Mode::Insert),
            "r" => Self::of(Mode::Replace),
            "v" => Self::of(Mode::Visual)
                .with(Self::of(Mode::VisualLine))
                .with(Self::of(Mode::VisualBlock)),
            "x" => Self::of(Mode::Visual).with(Self::of(Mode::VisualLine)),
            "b" => Self::of(Mode::VisualBlock),
            "s" => Self::of(Mode::Select),
            "o" => Self::of(Mode::OperatorPending),
            "c" => Self::of(Mode::Command),
            "t" => Self::of(Mode::Terminal),
            "a" => Self::ALL,
            _ => return None,
        })
    }

    /// Every mode there is.
    pub const ALL: Self = Self(
        Self::of(Mode::Normal).0
            | Self::of(Mode::Insert).0
            | Self::of(Mode::Replace).0
            | Self::of(Mode::Visual).0
            | Self::of(Mode::VisualLine).0
            | Self::of(Mode::VisualBlock).0
            | Self::of(Mode::Select).0
            | Self::of(Mode::OperatorPending).0
            | Self::of(Mode::Command).0
            | Self::of(Mode::Terminal).0,
    );
}

impl std::fmt::Debug for ModeSet {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "ModeSet({:#b})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{Mode, ModeSet};

    #[test]
    fn a_letter_stands_for_the_modes_vim_says_it_does() {
        let visual = ModeSet::from_letter("v").expect("v is a mode letter");
        assert!(visual.has(Mode::Visual));
        assert!(visual.has(Mode::VisualLine));
        assert!(visual.has(Mode::VisualBlock));
        assert!(!visual.has(Mode::Normal));

        let charwise = ModeSet::from_letter("x").expect("x is a mode letter");
        assert!(charwise.has(Mode::Visual));
        assert!(
            !charwise.has(Mode::VisualBlock),
            "a block is what `x` is not"
        );
    }

    #[test]
    fn everything_is_in_all() {
        for mode in [
            Mode::Normal,
            Mode::Insert,
            Mode::Replace,
            Mode::Visual,
            Mode::VisualLine,
            Mode::VisualBlock,
            Mode::Select,
            Mode::OperatorPending,
            Mode::Command,
            Mode::Terminal,
        ] {
            assert!(ModeSet::ALL.has(mode), "{mode:?}");
        }
        assert_eq!(ModeSet::from_letter("a"), Some(ModeSet::ALL));
    }

    #[test]
    fn a_letter_that_means_nothing_is_not_a_mode() {
        assert_eq!(ModeSet::from_letter("q"), None);
        assert_eq!(ModeSet::from_letter("normal"), None);
        assert_eq!(ModeSet::from_letter(""), None);
    }

    #[test]
    fn the_visual_modes_are_the_visual_modes() {
        assert!(Mode::Visual.is_visual());
        assert!(Mode::VisualBlock.is_visual());
        assert!(!Mode::Select.is_visual());
        assert!(Mode::Insert.is_inserting());
        assert!(Mode::Replace.is_inserting());
        assert!(!Mode::Normal.is_inserting());
    }

    #[test]
    fn the_status_line_has_a_word_and_a_colour_for_every_mode() {
        // A modal editor that could not say which mode it was in would be unusable, so this is
        // the one place a missing arm has to be impossible rather than merely unlikely.
        for mode in [
            Mode::Normal,
            Mode::Insert,
            Mode::Replace,
            Mode::Visual,
            Mode::VisualLine,
            Mode::VisualBlock,
            Mode::Select,
            Mode::OperatorPending,
            Mode::Command,
            Mode::Terminal,
        ] {
            assert!(!mode.label().is_empty(), "{mode:?}");
            assert!(!mode.tone().is_empty(), "{mode:?}");
        }
    }
}
