//! Key sequences, as they are written in a keymap.
//!
//! Vim's notation, because it is what the people who will write these files already know:
//! `<Leader>ff`, `<C-w>h`, `gd`, `<S-CR>`, `|`. Angle brackets name a key or hold modifiers;
//! everything else is the character it is.
//!
//! Parsing and formatting are inverses, which is what lets which-key show a pending sequence back
//! to the person typing it in the same words their configuration used.

use std::fmt::Write as _;

use crate::chord::{Chord, Key, Mods, Named};

/// What went wrong reading a key sequence.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum NotationError {
    /// A `<` with no `>` after it.
    #[error("unclosed `<` in {sequence:?}")]
    Unclosed {
        /// What was being read.
        sequence: String,
    },
    /// A `<...>` this notation has no meaning for.
    #[error("unknown key `<{name}>`")]
    UnknownKey {
        /// What was between the brackets.
        name: String,
    },
    /// A modifier with nothing after it, as in `<C->`.
    #[error("`<{name}>` names a modifier and no key")]
    NoKey {
        /// What was between the brackets.
        name: String,
    },
    /// Nothing at all.
    #[error("an empty key sequence")]
    Empty,
}

/// What `<Leader>` and `<LocalLeader>` stand for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Leaders {
    /// What `<Leader>` is. Space, the way almost every configuration sets it.
    pub leader: Chord,
    /// What `<LocalLeader>` is.
    pub local: Chord,
}

impl Default for Leaders {
    fn default() -> Self {
        Self {
            leader: Chord::named(Named::Space),
            local: Chord::char(','),
        }
    }
}

/// Reads a key sequence.
///
/// ```
/// use zdt_vim::chord::{Chord, Mods, Named};
/// use zdt_vim::notation::{Leaders, parse};
///
/// let keys = parse("<Leader>ff", Leaders::default()).unwrap();
/// assert_eq!(keys[0], Chord::named(Named::Space));
/// assert_eq!(keys[1], Chord::char('f'));
///
/// assert_eq!(parse("<C-w>", Leaders::default()).unwrap()[0], Chord::control('w'));
/// ```
pub fn parse(sequence: &str, leaders: Leaders) -> Result<Vec<Chord>, NotationError> {
    if sequence.is_empty() {
        return Err(NotationError::Empty);
    }

    let mut chords = Vec::new();
    let mut rest = sequence;

    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('<') {
            let Some(end) = after.find('>') else {
                return Err(NotationError::Unclosed {
                    sequence: sequence.to_owned(),
                });
            };
            let name = &after[..end];
            chords.push(bracketed(name, leaders)?);
            rest = &after[end + 1..];
            continue;
        }

        let character = rest.chars().next().expect("the rest is not empty");
        chords.push(Chord::char(character));
        rest = &rest[character.len_utf8()..];
    }

    Ok(chords)
}

/// One `<...>`.
fn bracketed(name: &str, leaders: Leaders) -> Result<Chord, NotationError> {
    match name.to_ascii_lowercase().as_str() {
        "leader" => return Ok(leaders.leader),
        "localleader" => return Ok(leaders.local),
        // The two characters the notation itself uses, so a keymap can bind them.
        "lt" => return Ok(Chord::char('<')),
        "gt" => return Ok(Chord::char('>')),
        "bar" => return Ok(Chord::char('|')),
        "bslash" => return Ok(Chord::char('\\')),
        "nop" | "nul" => {
            return Err(NotationError::UnknownKey {
                name: name.to_owned(),
            });
        }
        _ => {}
    }

    // Modifiers, in any order and any case: `<C-S-Tab>`, `<c-s-tab>`.
    let mut mods = Mods::NONE;
    let mut rest = name;
    while let Some((prefix, after)) = rest.split_once('-') {
        let modifier = match prefix.to_ascii_lowercase().as_str() {
            "c" | "ctrl" | "control" => Mods::CONTROL,
            "m" | "a" | "alt" | "meta" => Mods::ALT,
            "d" | "cmd" | "super" => Mods::SUPER,
            "s" | "shift" => Mods::SHIFT,
            // Not a modifier, so the rest is the key — which is how `g-` binds a hyphen.
            _ => break,
        };
        mods = mods.with(modifier);
        rest = after;
    }

    if rest.is_empty() {
        return Err(NotationError::NoKey {
            name: name.to_owned(),
        });
    }

    if let Some(named) = Named::from_str(rest) {
        return Ok(Chord::new(Key::Named(named), mods));
    }

    let mut characters = rest.chars();
    let character = characters.next().expect("the rest is not empty");
    if characters.next().is_some() {
        return Err(NotationError::UnknownKey {
            name: name.to_owned(),
        });
    }

    // A character key carries its shift in the character. `<S-a>` is `A`, so that a press and a
    // binding agree however the sequence was written.
    if mods.contains(Mods::SHIFT) {
        let shifted = character.to_ascii_uppercase();
        return Ok(Chord::new(Key::Char(shifted), mods.without(Mods::SHIFT)));
    }
    Ok(Chord::new(Key::Char(character), mods))
}

/// Writes a key sequence back out, the way a keymap would have written it.
///
/// Not through the leader: which-key shows what was typed, and a person who has just pressed
/// space is looking for what space did rather than for the word "leader".
#[must_use]
pub fn format(chords: &[Chord]) -> String {
    let mut out = String::new();
    for chord in chords {
        format_one(*chord, &mut out);
    }
    out
}

/// One chord, appended.
fn format_one(chord: Chord, out: &mut String) {
    let mut prefix = String::new();
    if chord.mods.contains(Mods::CONTROL) {
        prefix.push_str("C-");
    }
    if chord.mods.contains(Mods::ALT) {
        prefix.push_str("M-");
    }
    if chord.mods.contains(Mods::SUPER) {
        prefix.push_str("D-");
    }
    if chord.mods.contains(Mods::SHIFT) {
        prefix.push_str("S-");
    }

    match chord.key {
        Key::Named(named) => {
            let _ = write!(out, "<{prefix}{}>", named.as_str());
        }
        Key::Char(character) if !prefix.is_empty() => {
            let _ = write!(out, "<{prefix}{character}>");
        }
        // The two the notation would otherwise eat.
        Key::Char('<') => out.push_str("<lt>"),
        Key::Char('|') => out.push_str("<Bar>"),
        Key::Char(character) => out.push(character),
    }
}

#[cfg(test)]
mod tests {
    use crate::chord::{Chord, Key, Mods, Named};
    use crate::notation::{Leaders, NotationError, format, parse};

    fn read(sequence: &str) -> Vec<Chord> {
        parse(sequence, Leaders::default()).expect("the sequence reads")
    }

    #[test]
    fn plain_characters_are_themselves() {
        assert_eq!(read("gd"), vec![Chord::char('g'), Chord::char('d')]);
        assert_eq!(read("iw"), vec![Chord::char('i'), Chord::char('w')]);
    }

    #[test]
    fn the_leader_is_substituted_where_it_stands() {
        assert_eq!(
            read("<Leader>ff"),
            vec![
                Chord::named(Named::Space),
                Chord::char('f'),
                Chord::char('f')
            ]
        );
        assert_eq!(read("<LocalLeader>a")[0], Chord::char(','));
    }

    #[test]
    fn a_different_leader_changes_what_the_word_means() {
        let leaders = Leaders {
            leader: Chord::char(';'),
            ..Leaders::default()
        };
        assert_eq!(
            parse("<Leader>w", leaders).unwrap()[0],
            Chord::char(';'),
            "the keymap says `<Leader>`, the configuration says what that is"
        );
    }

    #[test]
    fn modifiers_are_read_in_any_order_and_any_case() {
        assert_eq!(read("<C-w>"), vec![Chord::control('w')]);
        assert_eq!(read("<c-w>"), vec![Chord::control('w')]);
        assert_eq!(read("<Ctrl-w>"), vec![Chord::control('w')]);
        assert_eq!(
            read("<C-M-x>"),
            vec![Chord::new(Key::Char('x'), Mods::CONTROL | Mods::ALT)]
        );
        assert_eq!(
            read("<M-C-x>"),
            vec![Chord::new(Key::Char('x'), Mods::CONTROL | Mods::ALT)]
        );
    }

    #[test]
    fn shift_on_a_letter_is_the_capital() {
        // Written either way, it is the same press — which is what makes `A` and `<S-a>` one
        // binding rather than two that shadow each other.
        assert_eq!(read("<S-a>"), read("A"));
    }

    #[test]
    fn shift_on_a_named_key_stays_beside_it() {
        assert_eq!(
            read("<S-CR>"),
            vec![Chord::new(Key::Named(Named::Enter), Mods::SHIFT)]
        );
    }

    #[test]
    fn the_notations_own_characters_can_be_bound() {
        assert_eq!(read("<lt>"), vec![Chord::char('<')]);
        assert_eq!(read("<Bar>"), vec![Chord::char('|')]);
        assert_eq!(read("|"), vec![Chord::char('|')]);
        assert_eq!(read("\\"), vec![Chord::char('\\')]);
    }

    #[test]
    fn a_sequence_reads_back_as_what_it_was() {
        for sequence in [
            "gd", "<C-w>h", "<Esc>", "<S-CR>", "]b", "<F7>", "<M-]>", "<lt>", "<Bar>", "ZZ",
        ] {
            assert_eq!(format(&read(sequence)), sequence, "{sequence}");
        }
    }

    #[test]
    fn the_leader_reads_back_as_the_key_it_is() {
        // Which-key shows what was pressed, not the word the configuration used for it.
        assert_eq!(format(&read("<Leader>ff")), "<Space>ff");
    }

    #[test]
    fn what_cannot_be_read_says_why() {
        let leaders = Leaders::default();
        assert!(matches!(
            parse("<C-w", leaders),
            Err(NotationError::Unclosed { .. })
        ));
        assert!(matches!(
            parse("<Nonsense>", leaders),
            Err(NotationError::UnknownKey { .. })
        ));
        assert!(matches!(
            parse("<C->", leaders),
            Err(NotationError::NoKey { .. })
        ));
        assert_eq!(parse("", leaders), Err(NotationError::Empty));
    }

    #[test]
    fn a_multi_byte_character_is_one_chord() {
        assert_eq!(read("ü"), vec![Chord::char('ü')]);
        assert_eq!(read("aü"), vec![Chord::char('a'), Chord::char('ü')]);
    }
}
