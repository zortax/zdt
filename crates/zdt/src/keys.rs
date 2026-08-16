//! Turning what the framework reports into what the keymap is written in.
//!
//! The one place the two vocabularies meet. Everything above it works in [`zdt_vim::Chord`], which
//! is why the whole grammar can be driven by a test that writes down what somebody typed.
//!
//! # Which of the two keys an event carries
//!
//! A key event says both what the key means with the modifiers applied, and what it would mean
//! without them. A shortcut wants the second, because `<C-w>` has to be control-and-w whatever the
//! layout does with control. Text wants the first, because that is the character the person meant.
//! The rule here: anything carrying control, alt or the platform key is a shortcut, and everything
//! else is what it looks like.

use zdt_vim::chord::{Chord, Key as VimKey, Mods, Named};
use zgui::vocab::{Key, KeyEvent, Modifiers, NamedKey};

/// The chord `event` is, when it is one.
///
/// Nothing for a modifier pressed on its own, for a dead key, or for a key the platform could not
/// identify. A keymap can be written against none of them.
#[must_use]
pub fn chord_of(event: &KeyEvent, modifiers: Modifiers) -> Option<Chord> {
    let mut mods = Mods::NONE;
    if modifiers.control() {
        mods = mods.with(Mods::CONTROL);
    }
    if modifiers.alt() {
        mods = mods.with(Mods::ALT);
    }
    if modifiers.meta() {
        mods = mods.with(Mods::SUPER);
    }

    // A shortcut is matched against the key the layout would give with nothing held, so that
    // `<C-w>` is control-and-w on every keyboard.
    let shortcut = !mods.is_empty();
    let key = if shortcut {
        &event.key_without_modifiers
    } else {
        &event.key
    };

    let vim = match key {
        Key::Named(named) => {
            let named = named_of(*named)?;
            // Shift is only a modifier for a key that has a name; on a character it is already in
            // the character.
            if modifiers.shift() {
                mods = mods.with(Mods::SHIFT);
            }
            VimKey::Named(named)
        }
        Key::Character(text) => {
            let mut characters = text.chars();
            let character = characters.next()?;
            if characters.next().is_some() {
                // One press that produced several characters is text, never a command.
                return None;
            }
            if character.is_control() {
                return None;
            }
            // A shortcut's letter is written lowercase in a keymap, and shift beside it is what
            // tells `<C-S-w>` from `<C-w>`.
            if shortcut {
                if modifiers.shift() {
                    mods = mods.with(Mods::SHIFT);
                }
                VimKey::Char(character.to_ascii_lowercase())
            } else {
                VimKey::Char(character)
            }
        }
        // `Key` is open to new kinds; anything this has no word for is not a chord, which is
        // safer than a match that would have to guess.
        _ => return None,
    };

    Some(Chord::new(vim, mods))
}

/// The named key `named` is, when the keymap has a name for it.
fn named_of(named: NamedKey) -> Option<Named> {
    Some(match named {
        NamedKey::Escape => Named::Escape,
        NamedKey::Enter => Named::Enter,
        NamedKey::Tab => Named::Tab,
        NamedKey::Backspace => Named::Backspace,
        NamedKey::Delete => Named::Delete,
        NamedKey::Insert => Named::Insert,
        NamedKey::Space => Named::Space,
        NamedKey::ArrowLeft => Named::Left,
        NamedKey::ArrowRight => Named::Right,
        NamedKey::ArrowUp => Named::Up,
        NamedKey::ArrowDown => Named::Down,
        NamedKey::Home => Named::Home,
        NamedKey::End => Named::End,
        NamedKey::PageUp => Named::PageUp,
        NamedKey::PageDown => Named::PageDown,
        NamedKey::F1 => Named::Function(1),
        NamedKey::F2 => Named::Function(2),
        NamedKey::F3 => Named::Function(3),
        NamedKey::F4 => Named::Function(4),
        NamedKey::F5 => Named::Function(5),
        NamedKey::F6 => Named::Function(6),
        NamedKey::F7 => Named::Function(7),
        NamedKey::F8 => Named::Function(8),
        NamedKey::F9 => Named::Function(9),
        NamedKey::F10 => Named::Function(10),
        NamedKey::F11 => Named::Function(11),
        NamedKey::F12 => Named::Function(12),
        // A modifier on its own is no chord, and neither is anything the keymap has no word for.
        // Both reach here, and both answer nothing. A key that matched wrongly would be worse.
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use zdt_vim::chord::{Chord, Mods, Named};
    use zgui::vocab::{Key, KeyEvent, Modifiers, NamedKey, PhysicalKey};

    use super::chord_of;

    /// A press of a character key, as the platform reports one.
    fn typed(shifted: &str, bare: &str) -> KeyEvent {
        KeyEvent {
            key: Key::character(shifted),
            key_without_modifiers: Key::character(bare),
            physical: PhysicalKey::Unidentified(0),
            location: zgui::vocab::KeyLocation::Standard,
            repeat: false,
        }
    }

    /// A press of a named key.
    fn named(key: NamedKey) -> KeyEvent {
        KeyEvent::named(key, PhysicalKey::Unidentified(0))
    }

    #[test]
    fn a_plain_letter_is_itself() {
        let chord = chord_of(&typed("w", "w"), Modifiers::NONE).expect("w is a chord");
        assert_eq!(chord, Chord::char('w'));
    }

    #[test]
    fn a_capital_is_the_capital_rather_than_shift_and_a_letter() {
        // Which is what makes the keymap able to write `A` and `a` as two different rows.
        let chord = chord_of(&typed("A", "a"), Modifiers::SHIFT).expect("A is a chord");
        assert_eq!(chord, Chord::char('A'));
        assert_eq!(chord.mods, Mods::NONE);
    }

    #[test]
    fn a_shortcut_is_matched_against_the_unmodified_key() {
        // On a layout where control changes what the key produces, `<C-w>` still has to be w.
        let event = typed("\u{17}", "w");
        let chord = chord_of(&event, Modifiers::CONTROL).expect("it is a chord");
        assert_eq!(chord, Chord::control('w'));
    }

    #[test]
    fn a_shortcut_letter_is_lowered_so_the_keymap_can_write_it_once() {
        let chord = chord_of(&typed("W", "W"), Modifiers::CONTROL | Modifiers::SHIFT)
            .expect("it is a chord");
        assert_eq!(chord.key, zdt_vim::chord::Key::Char('w'));
        assert!(chord.mods.contains(Mods::CONTROL));
        assert!(chord.mods.contains(Mods::SHIFT));
    }

    #[test]
    fn shift_stays_beside_a_named_key() {
        let chord = chord_of(&named(NamedKey::Enter), Modifiers::SHIFT).expect("it is a chord");
        assert_eq!(chord.key, zdt_vim::chord::Key::Named(Named::Enter));
        assert!(chord.mods.contains(Mods::SHIFT));
    }

    #[test]
    fn the_space_bar_is_the_named_key_rather_than_a_character() {
        // The leader is space, and a leader that arrived as a bare character would never match.
        let chord = chord_of(&named(NamedKey::Space), Modifiers::NONE).expect("it is a chord");
        assert_eq!(chord, Chord::named(Named::Space));
    }

    #[test]
    fn a_modifier_on_its_own_is_not_a_chord() {
        for key in [
            NamedKey::Control,
            NamedKey::Shift,
            NamedKey::Alt,
            NamedKey::Meta,
        ] {
            assert_eq!(chord_of(&named(key), Modifiers::NONE), None, "{key:?}");
        }
    }

    #[test]
    fn a_dead_key_is_not_a_chord() {
        let event = KeyEvent {
            key: Key::Dead(Some('\u{300}')),
            key_without_modifiers: Key::Dead(Some('\u{300}')),
            physical: PhysicalKey::Unidentified(0),
            location: zgui::vocab::KeyLocation::Standard,
            repeat: false,
        };
        assert_eq!(chord_of(&event, Modifiers::NONE), None);
    }

    #[test]
    fn a_key_that_produced_several_characters_is_text() {
        // A ligature or a composed character is something to insert, never a command.
        assert_eq!(chord_of(&typed("ffi", "ffi"), Modifiers::NONE), None);
    }

    #[test]
    fn the_function_keys_are_named() {
        let chord = chord_of(&named(NamedKey::F7), Modifiers::NONE).expect("it is a chord");
        assert_eq!(chord, Chord::named(Named::Function(7)));
    }

    #[test]
    fn a_multi_byte_character_survives() {
        let chord = chord_of(&typed("ü", "ü"), Modifiers::NONE).expect("it is a chord");
        assert_eq!(chord, Chord::char('ü'));
    }
}
