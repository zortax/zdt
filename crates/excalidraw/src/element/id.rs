//! What names an element, and what its wobble is drawn from.

use std::fmt;

/// Which element this is.
///
/// A file's own ids are 21 characters of the alphabet below, but nothing reads them, so an id read
/// from a file is kept exactly as it was written.
#[derive(Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct Id(pub String);

/// The letters a fresh id is made of.
const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-";

/// How long one is.
const LENGTH: usize = 21;

impl Id {
    /// The id `text` names.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// It, as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// A fresh id, from `random`.
    ///
    /// The generator is the caller's, so a session that must be repeatable can hand in one it
    /// controls.
    #[must_use]
    pub fn fresh(random: &mut excalidraw_rough::Random) -> Self {
        let mut text = String::with_capacity(LENGTH);
        for _ in 0..LENGTH {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let at = (random.next() * ALPHABET.len() as f64) as usize;
            text.push(char::from(ALPHABET[at.min(ALPHABET.len() - 1)]));
        }
        Self(text)
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for Id {
    fn from(text: &str) -> Self {
        Self(text.to_owned())
    }
}

impl From<String> for Id {
    fn from(text: String) -> Self {
        Self(text)
    }
}

/// What one element's wobble is drawn from.
///
/// Held apart from a plain number so that it cannot be confused with the other integers on an
/// element, of which there are several.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Seed(pub u32);

/// The largest seed a file holds. rough.js draws from a 31-bit number.
const SEED_LIMIT: f64 = 2_147_483_648.0;

impl Seed {
    /// A fresh seed, from `random`.
    #[must_use]
    pub fn fresh(random: &mut excalidraw_rough::Random) -> Self {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Self((random.next() * SEED_LIMIT) as u32)
    }

    /// The seed `value` names, whatever the file put there.
    #[must_use]
    pub fn from_number(value: f64) -> Self {
        if value.is_finite() && value >= 0.0 {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            Self((value % SEED_LIMIT) as u32)
        } else {
            // rough.js draws differently every time from a seed of nothing, so a file that has
            // none gets the one Excalidraw's own reader gives it.
            Self(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_id_is_the_right_length_and_alphabet() {
        let mut random = excalidraw_rough::Random::new(1);
        let id = Id::fresh(&mut random);
        assert_eq!(id.as_str().chars().count(), LENGTH);
        assert!(id.as_str().bytes().all(|letter| ALPHABET.contains(&letter)));
    }

    #[test]
    fn two_fresh_ids_from_one_generator_differ() {
        let mut random = excalidraw_rough::Random::new(1);
        assert_ne!(Id::fresh(&mut random), Id::fresh(&mut random));
    }

    #[test]
    fn a_seed_of_nothing_becomes_one() {
        assert_eq!(Seed::from_number(f64::NAN), Seed(1));
        assert_eq!(Seed::from_number(-5.0), Seed(1));
        assert_eq!(Seed::from_number(1_263_748_391.0), Seed(1_263_748_391));
    }
}
