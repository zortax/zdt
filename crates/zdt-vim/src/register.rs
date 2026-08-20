//! The named clipboards.
//!
//! Vim's registers. They are the reason `dd` then `p` puts the line back, and `"ayy` then `"ap`
//! puts a different one somewhere else. What matters and is easy to get wrong:
//!
//! * a yank goes to `0` as well as to the unnamed register, so `y` then `d` then `p` still pastes
//!   what was yanked when `"0p` asks for it.
//! * a delete pushes the numbered registers along, so `"1p` is the last delete and `"2p` the one
//!   before it.
//! * an uppercase name appends, which is how a run of `"Ayy` collects lines.
//! * whether the text was taken by lines decides whether pasting opens a line or inserts inline.

use rustc_hash::FxHashMap;

/// What is in one register.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Contents {
    /// The text.
    pub text: String,
    /// Whether it was taken as whole lines, which decides how pasting puts it back.
    pub linewise: bool,
}

impl Contents {
    /// Text taken character by character.
    #[must_use]
    pub fn charwise(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            linewise: false,
        }
    }

    /// Text taken as whole lines.
    #[must_use]
    pub fn linewise(text: impl Into<String>) -> Self {
        let mut text = text.into();
        // A linewise register always ends in a break, so pasting one is putting a line in rather
        // than joining what was taken onto whatever it lands next to.
        if !text.ends_with('\n') {
            text.push('\n');
        }
        Self {
            text,
            linewise: true,
        }
    }
}

/// Which register something is going to or coming from.
///
/// The unnamed one when nothing said otherwise, which is what almost every yank and delete uses.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Name(Option<char>);

impl Name {
    /// The unnamed register.
    pub const UNNAMED: Self = Self(None);

    /// The register written as `character`, when it is one.
    ///
    /// `a` to `z` and their capitals, `0` to `9`, `+` and `*` for the system clipboards, `_` for
    /// the one that throws away, and `"` which is the unnamed one written out.
    #[must_use]
    pub fn of(character: char) -> Option<Self> {
        if character == '"' {
            return Some(Self::UNNAMED);
        }
        let known = character.is_ascii_alphabetic()
            || character.is_ascii_digit()
            // `-` is the small-delete register, and the rest are the read-only ones the editor
            // keeps up to date.
            || matches!(character, '+' | '*' | '_' | '-' | '.' | '%' | ':');
        known.then_some(Self(Some(character)))
    }

    /// What it is written as.
    #[must_use]
    pub fn character(self) -> char {
        self.0.unwrap_or('"')
    }

    /// Whether this is one of the system clipboards.
    #[must_use]
    pub fn is_clipboard(self) -> bool {
        matches!(self.0, Some('+' | '*'))
    }

    /// Whether this is the one that throws away what it is given.
    #[must_use]
    pub fn is_black_hole(self) -> bool {
        self.0 == Some('_')
    }

    /// Whether writing to this appends. It replaces otherwise.
    #[must_use]
    pub fn appends(self) -> bool {
        self.0.is_some_and(|name| name.is_ascii_uppercase())
    }

    /// The lowercase register an uppercase name writes into.
    #[must_use]
    pub fn canonical(self) -> Self {
        match self.0 {
            Some(name) if name.is_ascii_uppercase() => Self(Some(name.to_ascii_lowercase())),
            other => Self(other),
        }
    }
}

/// Every register.
#[derive(Debug, Default)]
pub struct Registers {
    held: FxHashMap<char, Contents>,
    unnamed: Contents,
}

impl Registers {
    /// Nothing in any of them.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// What is in `name`.
    #[must_use]
    pub fn get(&self, name: Name) -> Contents {
        match name.canonical().0 {
            None => self.unnamed.clone(),
            Some(character) => self.held.get(&character).cloned().unwrap_or_default(),
        }
    }

    /// Puts `contents` in `name`, appending when the name is uppercase.
    ///
    /// A named register is also the unnamed one afterwards. So `"ayy` then `p` pastes what was
    /// just yanked.
    pub fn set(&mut self, name: Name, contents: Contents) {
        if name.is_black_hole() {
            return;
        }
        let canonical = name.canonical();
        let contents = if name.appends() {
            let mut held = self.get(canonical);
            if held.linewise && !held.text.ends_with('\n') {
                held.text.push('\n');
            }
            held.text.push_str(&contents.text);
            Contents {
                linewise: held.linewise || contents.linewise,
                text: held.text,
            }
        } else {
            contents
        };

        match canonical.0 {
            None => self.unnamed = contents,
            Some(character) => {
                self.held.insert(character, contents.clone());
                self.unnamed = contents;
            }
        }
    }

    /// Records a yank: into `name`, and into `0` when the yank did not name one.
    ///
    /// The reason `"0p` pastes the last *yank* even after a delete has overwritten the unnamed
    /// register. It is the single most useful thing about vim's registers.
    pub fn yank(&mut self, name: Name, contents: Contents) {
        if name == Name::UNNAMED {
            self.held.insert('0', contents.clone());
        }
        self.set(name, contents);
    }

    /// Records a delete: into `name`, and into the numbered ring when it did not name one.
    ///
    /// A small delete, meaning less than a line and taken charwise, goes to `-` and leaves the
    /// ring alone. That stops a run of `x` from throwing away nine lines of history.
    pub fn delete(&mut self, name: Name, contents: Contents) {
        if name == Name::UNNAMED {
            if contents.linewise || contents.text.contains('\n') {
                for number in (1..9u8).rev() {
                    let from = (b'0' + number) as char;
                    let to = (b'0' + number + 1) as char;
                    if let Some(held) = self.held.get(&from).cloned() {
                        self.held.insert(to, held);
                    }
                }
                self.held.insert('1', contents.clone());
            } else {
                self.held.insert('-', contents.clone());
            }
        }
        self.set(name, contents);
    }

    /// Puts something in a register without it becoming the unnamed one.
    ///
    /// For the read-only registers the editor keeps up to date: the file's name, the last inserted
    /// text, the last command.
    pub fn set_quietly(&mut self, character: char, contents: Contents) {
        self.held.insert(character, contents);
    }

    /// What is in the unnamed register.
    #[must_use]
    pub fn unnamed(&self) -> &Contents {
        &self.unnamed
    }

    /// Puts every register back, which restoring a session does.
    ///
    /// Quiet: setting one the ordinary way shifts the numbered registers along, and putting a
    /// saved set back is not a yank.
    pub fn restore(&mut self, unnamed: Contents, held: impl IntoIterator<Item = (char, Contents)>) {
        self.unnamed = unnamed;
        self.held = held.into_iter().collect();
    }

    /// Every register with something in it, for the picker that lists them.
    #[must_use]
    pub fn occupied(&self) -> Vec<(char, &Contents)> {
        let mut all: Vec<(char, &Contents)> = self
            .held
            .iter()
            .filter(|(_, contents)| !contents.text.is_empty())
            .map(|(name, contents)| (*name, contents))
            .collect();
        all.sort_by_key(|(name, _)| *name);
        if !self.unnamed.text.is_empty() {
            all.insert(0, ('"', &self.unnamed));
        }
        all
    }
}

#[cfg(test)]
mod tests {
    use super::{Contents, Name, Registers};

    #[test]
    fn a_yank_goes_to_the_unnamed_register_and_to_zero() {
        // Which is what makes `"0p` paste the last yank even after a delete.
        let mut registers = Registers::new();
        registers.yank(Name::UNNAMED, Contents::charwise("yanked"));
        registers.delete(Name::UNNAMED, Contents::linewise("deleted"));

        assert_eq!(registers.get(Name::UNNAMED).text, "deleted\n");
        assert_eq!(
            registers.get(Name::of('0').unwrap()).text,
            "yanked",
            "the yank is still there"
        );
    }

    #[test]
    fn deletes_push_the_numbered_ring_along() {
        let mut registers = Registers::new();
        registers.delete(Name::UNNAMED, Contents::linewise("first"));
        registers.delete(Name::UNNAMED, Contents::linewise("second"));
        registers.delete(Name::UNNAMED, Contents::linewise("third"));

        assert_eq!(registers.get(Name::of('1').unwrap()).text, "third\n");
        assert_eq!(registers.get(Name::of('2').unwrap()).text, "second\n");
        assert_eq!(registers.get(Name::of('3').unwrap()).text, "first\n");
    }

    #[test]
    fn a_small_delete_does_not_push_the_ring() {
        // Otherwise nine presses of `x` would throw away every line of delete history.
        let mut registers = Registers::new();
        registers.delete(Name::UNNAMED, Contents::linewise("a line"));
        registers.delete(Name::UNNAMED, Contents::charwise("x"));

        assert_eq!(registers.get(Name::of('1').unwrap()).text, "a line\n");
        assert_eq!(registers.get(Name::of('-').unwrap()).text, "x");
    }

    #[test]
    fn a_named_register_is_the_unnamed_one_afterwards() {
        let mut registers = Registers::new();
        registers.yank(Name::of('a').unwrap(), Contents::charwise("into a"));
        assert_eq!(registers.get(Name::of('a').unwrap()).text, "into a");
        assert_eq!(registers.get(Name::UNNAMED).text, "into a");
    }

    #[test]
    fn a_capital_name_appends() {
        let mut registers = Registers::new();
        registers.yank(Name::of('a').unwrap(), Contents::linewise("one"));
        registers.yank(Name::of('A').unwrap(), Contents::linewise("two"));
        assert_eq!(registers.get(Name::of('a').unwrap()).text, "one\ntwo\n");
    }

    #[test]
    fn the_black_hole_keeps_nothing() {
        let mut registers = Registers::new();
        registers.yank(Name::UNNAMED, Contents::charwise("kept"));
        registers.delete(Name::of('_').unwrap(), Contents::charwise("thrown away"));
        assert_eq!(
            registers.get(Name::UNNAMED).text,
            "kept",
            "the unnamed register was not touched either"
        );
    }

    #[test]
    fn a_linewise_register_always_ends_in_a_break() {
        // Otherwise pasting it would join it onto whatever it landed next to.
        assert_eq!(Contents::linewise("no break").text, "no break\n");
        assert_eq!(Contents::linewise("has one\n").text, "has one\n");
    }

    #[test]
    fn only_the_names_that_mean_something_are_registers() {
        assert!(Name::of('a').is_some());
        assert!(Name::of('Z').is_some());
        assert!(Name::of('7').is_some());
        assert!(Name::of('+').is_some());
        assert!(Name::of('_').is_some());
        assert_eq!(Name::of('"'), Some(Name::UNNAMED));
        assert!(Name::of('§').is_none());
        assert!(Name::of(' ').is_none());
    }

    #[test]
    fn the_clipboards_are_told_apart() {
        assert!(Name::of('+').unwrap().is_clipboard());
        assert!(Name::of('*').unwrap().is_clipboard());
        assert!(!Name::of('a').unwrap().is_clipboard());
    }

    #[test]
    fn an_empty_register_reads_as_empty_rather_than_failing() {
        let registers = Registers::new();
        assert_eq!(registers.get(Name::of('q').unwrap()), Contents::default());
    }

    #[test]
    fn a_yank_that_named_a_register_does_not_touch_zero() {
        // Vim's rule: `0` holds the last yank *unless* the yank said where to put it.
        let mut registers = Registers::new();
        registers.yank(Name::UNNAMED, Contents::charwise("plain"));
        registers.yank(Name::of('a').unwrap(), Contents::charwise("into a"));
        assert_eq!(registers.get(Name::of('0').unwrap()).text, "plain");
    }

    #[test]
    fn the_occupied_registers_are_listed_for_the_picker() {
        let mut registers = Registers::new();
        registers.yank(Name::of('b').unwrap(), Contents::charwise("bee"));
        registers.yank(Name::of('a').unwrap(), Contents::charwise("ay"));
        let listed: Vec<char> = registers
            .occupied()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(listed, vec!['"', 'a', 'b']);
    }
}
