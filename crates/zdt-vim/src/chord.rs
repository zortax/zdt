//! One press of one key.
//!
//! The vocabulary the keymap is written in and the engine matches against. It is this crate's own
//! rather than the framework's for one reason: everything here has to be constructible in a test
//! from a string, so that the whole grammar can be driven by writing down what somebody typed.

use std::fmt;

/// A key that is not a character.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Named {
    /// Escape.
    Escape,
    /// Return, written `<CR>`.
    Enter,
    /// Tab.
    Tab,
    /// Backspace.
    Backspace,
    /// Delete.
    Delete,
    /// Insert.
    Insert,
    /// The space bar.
    ///
    /// A character key that has to be named, because a space in the middle of a key sequence is
    /// unreadable and because the leader is one.
    Space,
    /// The arrow keys.
    Left,
    /// The arrow keys.
    Right,
    /// The arrow keys.
    Up,
    /// The arrow keys.
    Down,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// A function key, counting from one.
    Function(u8),
}

impl Named {
    /// How this is written in a key sequence, without its angle brackets.
    #[must_use]
    pub fn as_str(self) -> String {
        match self {
            Self::Escape => "Esc".to_owned(),
            Self::Enter => "CR".to_owned(),
            Self::Tab => "Tab".to_owned(),
            Self::Backspace => "BS".to_owned(),
            Self::Delete => "Del".to_owned(),
            Self::Insert => "Insert".to_owned(),
            Self::Space => "Space".to_owned(),
            Self::Left => "Left".to_owned(),
            Self::Right => "Right".to_owned(),
            Self::Up => "Up".to_owned(),
            Self::Down => "Down".to_owned(),
            Self::Home => "Home".to_owned(),
            Self::End => "End".to_owned(),
            Self::PageUp => "PageUp".to_owned(),
            Self::PageDown => "PageDown".to_owned(),
            Self::Function(number) => format!("F{number}"),
        }
    }

    /// The key written as `name`, when there is one. Case-insensitive, as vim's notation is.
    ///
    /// Not `FromStr`: that trait's error type would have to say something, and there is nothing
    /// to say beyond "no key is written that way".
    #[must_use]
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(name: &str) -> Option<Self> {
        let lowered = name.to_ascii_lowercase();
        Some(match lowered.as_str() {
            "esc" | "escape" => Self::Escape,
            "cr" | "enter" | "return" => Self::Enter,
            "tab" => Self::Tab,
            "bs" | "backspace" => Self::Backspace,
            "del" | "delete" => Self::Delete,
            "insert" => Self::Insert,
            "space" => Self::Space,
            "left" => Self::Left,
            "right" => Self::Right,
            "up" => Self::Up,
            "down" => Self::Down,
            "home" => Self::Home,
            "end" => Self::End,
            "pageup" | "pgup" => Self::PageUp,
            "pagedown" | "pgdn" => Self::PageDown,
            other => {
                let number = other.strip_prefix('f')?.parse::<u8>().ok()?;
                if (1..=24).contains(&number) {
                    Self::Function(number)
                } else {
                    return None;
                }
            }
        })
    }
}

/// What a key means.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Key {
    /// A key that produces a character, as the layout and shift leave it.
    ///
    /// Shift is *in* the character rather than beside it: `A` is one binding and `a` is another,
    /// and a keymap that had to say `<S-a>` for the first would be unreadable.
    Char(char),
    /// A key that has a name instead.
    Named(Named),
}

/// The modifiers held with a key.
///
/// Shift is only here for a named key. On a character key the layout has already applied it, and
/// carrying it as well would make `A` and `<S-a>` two different bindings for one press.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Mods(u8);

impl Mods {
    /// Nothing held.
    pub const NONE: Self = Self(0);
    /// Either control key.
    pub const CONTROL: Self = Self(1 << 0);
    /// Either alt key, called option on macOS.
    pub const ALT: Self = Self(1 << 1);
    /// The platform's command modifier: Super, Command or the Windows key.
    pub const SUPER: Self = Self(1 << 2);
    /// Either shift key, on a key that has a name.
    pub const SHIFT: Self = Self(1 << 3);

    /// The set with these bits.
    #[must_use]
    pub const fn from_bits(bits: u8) -> Self {
        Self(bits & 0b1111)
    }

    /// The raw bits.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Whether every modifier in `other` is held.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether nothing is held.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The same set with `other` added.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// The same set with `other` taken out.
    #[must_use]
    pub const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

impl std::ops::BitOr for Mods {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        self.with(other)
    }
}

impl fmt::Debug for Mods {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut names = Vec::new();
        if self.contains(Self::CONTROL) {
            names.push("C");
        }
        if self.contains(Self::ALT) {
            names.push("A");
        }
        if self.contains(Self::SUPER) {
            names.push("D");
        }
        if self.contains(Self::SHIFT) {
            names.push("S");
        }
        write!(formatter, "Mods({})", names.join("-"))
    }
}

/// One press: a key and what was held with it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Chord {
    /// What the key means.
    pub key: Key,
    /// What was held.
    pub mods: Mods,
}

impl Chord {
    /// A press of `key` with `mods`.
    #[must_use]
    pub const fn new(key: Key, mods: Mods) -> Self {
        Self { key, mods }
    }

    /// A bare character.
    #[must_use]
    pub const fn char(character: char) -> Self {
        Self::new(Key::Char(character), Mods::NONE)
    }

    /// A bare named key.
    #[must_use]
    pub const fn named(named: Named) -> Self {
        Self::new(Key::Named(named), Mods::NONE)
    }

    /// A character with control held.
    #[must_use]
    pub const fn control(character: char) -> Self {
        Self::new(Key::Char(character), Mods::CONTROL)
    }

    /// The character this press inserts, when it inserts one.
    ///
    /// Nothing when anything but shift is held: a key with control on it is a command, and
    /// inserting the letter it happens to be would be a very surprising way to lose a file.
    #[must_use]
    pub fn inserted(self) -> Option<char> {
        if !self.mods.without(Mods::SHIFT).is_empty() {
            return None;
        }
        match self.key {
            Key::Char(character) => Some(character),
            Key::Named(Named::Space) => Some(' '),
            Key::Named(Named::Tab) => Some('\t'),
            Key::Named(_) => None,
        }
    }

    /// Whether this is a digit with nothing held, which is what a count is made of.
    #[must_use]
    pub fn digit(self) -> Option<u32> {
        if !self.mods.is_empty() {
            return None;
        }
        match self.key {
            Key::Char(character) => character.to_digit(10),
            Key::Named(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Chord, Key, Mods, Named};

    #[test]
    fn shift_lives_in_the_character_rather_than_beside_it() {
        // Otherwise `A` and `<S-a>` would be two bindings for one press, and a keymap would have
        // to say which one it meant.
        let shouted = Chord::char('A');
        assert_eq!(shouted.mods, Mods::NONE);
        assert_ne!(shouted, Chord::char('a'));
    }

    #[test]
    fn a_key_with_control_on_it_inserts_nothing() {
        assert_eq!(Chord::char('w').inserted(), Some('w'));
        assert_eq!(Chord::control('w').inserted(), None);
        assert_eq!(Chord::named(Named::Space).inserted(), Some(' '));
        assert_eq!(Chord::named(Named::Escape).inserted(), None);
    }

    #[test]
    fn only_a_bare_digit_is_a_count() {
        assert_eq!(Chord::char('5').digit(), Some(5));
        assert_eq!(Chord::control('5').digit(), None);
        assert_eq!(Chord::char('x').digit(), None);
    }

    #[test]
    fn modifiers_are_a_set() {
        let both = Mods::CONTROL | Mods::ALT;
        assert!(both.contains(Mods::CONTROL));
        assert!(both.contains(Mods::ALT));
        assert!(!both.contains(Mods::SUPER));
        assert!(both.without(Mods::ALT) == Mods::CONTROL);
        assert!(Mods::NONE.is_empty());
    }

    #[test]
    fn every_named_key_reads_back_as_itself() {
        let all = [
            Named::Escape,
            Named::Enter,
            Named::Tab,
            Named::Backspace,
            Named::Delete,
            Named::Insert,
            Named::Space,
            Named::Left,
            Named::Right,
            Named::Up,
            Named::Down,
            Named::Home,
            Named::End,
            Named::PageUp,
            Named::PageDown,
            Named::Function(7),
        ];
        for key in all {
            assert_eq!(Named::from_str(&key.as_str()), Some(key), "{key:?}");
        }
    }

    #[test]
    fn a_name_is_read_whatever_its_case() {
        assert_eq!(Named::from_str("esc"), Some(Named::Escape));
        assert_eq!(Named::from_str("ESC"), Some(Named::Escape));
        assert_eq!(Named::from_str("f12"), Some(Named::Function(12)));
        assert_eq!(Named::from_str("F0"), None);
        assert_eq!(Named::from_str("nonsense"), None);
    }

    #[test]
    fn a_chord_is_a_key_and_what_was_held() {
        let chord = Chord::new(Key::Char('w'), Mods::CONTROL);
        assert_eq!(chord, Chord::control('w'));
    }
}
