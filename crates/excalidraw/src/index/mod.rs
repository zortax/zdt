//! The key that says where an element sits in the order.
//!
//! The array is the order; the key is how two people who reordered a drawing at the same time agree
//! on what it became. A key is a string that sorts: to put something between two elements, a key
//! between their two keys is made, and no other key has to move.
//!
//! The algorithm is the one Excalidraw vendors, over the base-62 digits below. Ported rather than
//! taken from a crate, because the only published Rust implementation with this alphabet has not
//! been touched since 2022, and the ones that are maintained use a byte-string format a file
//! written here could not be read with.

mod sync;

pub use self::sync::{sync_invalid, sync_moved};

/// The digits a key is written in, in the order they sort.
pub const DIGITS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Why a key could not be read or made.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum Error {
    /// It is not a key at all.
    #[error("not an order key: {0}")]
    NotAKey(String),
    /// The two keys given are not in order.
    #[error("{0} is not before {1}")]
    OutOfOrder(String, String),
    /// There is no key past this one in that direction.
    #[error("no key past {0}")]
    NoRoom(String),
}

/// Where in the digits `letter` sits.
fn digit(letter: u8) -> Option<usize> {
    DIGITS.iter().position(|held| *held == letter)
}

/// The first digit.
fn zero() -> char {
    char::from(DIGITS[0])
}

/// The last.
fn last_digit() -> char {
    char::from(DIGITS[DIGITS.len() - 1])
}

/// How long the whole-number part of a key beginning with `head` is.
fn integer_length(head: u8) -> Result<usize, Error> {
    match head {
        b'a'..=b'z' => Ok((head - b'a') as usize + 2),
        b'A'..=b'Z' => Ok((b'Z' - head) as usize + 2),
        _ => Err(Error::NotAKey(char::from(head).to_string())),
    }
}

/// The whole-number part of `key`.
fn integer_part(key: &str) -> Result<&str, Error> {
    let head = key
        .bytes()
        .next()
        .ok_or_else(|| Error::NotAKey(key.into()))?;
    let length = integer_length(head)?;
    if length > key.len() {
        return Err(Error::NotAKey(key.into()));
    }
    Ok(&key[..length])
}

/// Whether `key` is one this crate could have written.
///
/// # Errors
///
/// If it is not: a letter outside the digits, a whole-number part of the wrong length, a trailing
/// zero, or the one key the format reserves.
pub fn validate(key: &str) -> Result<(), Error> {
    let reserved = format!("A{}", zero().to_string().repeat(26));
    if key == reserved || !key.bytes().all(|letter| digit(letter).is_some()) {
        return Err(Error::NotAKey(key.into()));
    }
    let whole = integer_part(key)?;
    let fraction = &key[whole.len()..];
    if fraction.ends_with(zero()) {
        return Err(Error::NotAKey(key.into()));
    }
    Ok(())
}

/// Whether `key` is one at all, without saying why not.
#[must_use]
pub fn is_valid(key: &str) -> bool {
    validate(key).is_ok()
}

/// The shortest fraction between `a` and `b`.
fn midpoint(a: &str, b: Option<&str>) -> Result<String, Error> {
    let zero = zero();
    if let Some(b) = b
        && a >= b
    {
        return Err(Error::OutOfOrder(a.into(), b.into()));
    }
    if a.ends_with(zero) || b.is_some_and(|b| b.ends_with(zero)) {
        return Err(Error::NotAKey(a.into()));
    }

    if let Some(b) = b {
        // What the two share is kept, and the search goes on past it.
        let mut shared = 0;
        loop {
            let from_a = a.as_bytes().get(shared).copied().unwrap_or(DIGITS[0]);
            let from_b = b.as_bytes().get(shared).copied();
            if Some(from_a) != from_b {
                break;
            }
            shared += 1;
        }
        if shared > 0 {
            let rest = midpoint(&a[a.len().min(shared)..], Some(&b[shared..]))?;
            return Ok(format!("{}{rest}", &b[..shared]));
        }
    }

    let digit_a = a.bytes().next().and_then(digit).unwrap_or(0);
    let digit_b = match b {
        Some(b) => b.bytes().next().and_then(digit).unwrap_or(0),
        None => DIGITS.len(),
    };
    if digit_b - digit_a > 1 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let middle = (0.5 * (digit_a + digit_b) as f64).round() as usize;
        return Ok(char::from(DIGITS[middle]).to_string());
    }
    // The two digits are next to each other, so the answer is one digit longer.
    if let Some(b) = b
        && b.len() > 1
    {
        return Ok(b[..1].to_string());
    }
    let rest = midpoint(a.get(1..).unwrap_or(""), None)?;
    Ok(format!("{}{rest}", char::from(DIGITS[digit_a])))
}

/// The whole-number part one past `x`, when there is one.
fn increment(x: &str) -> Result<Option<String>, Error> {
    if x.len() != integer_length(x.as_bytes()[0])? {
        return Err(Error::NotAKey(x.into()));
    }
    let head = x.as_bytes()[0];
    let mut digits: Vec<u8> = x.bytes().skip(1).collect();
    let mut carry = true;
    for at in (0..digits.len()).rev() {
        if !carry {
            break;
        }
        let next = digit(digits[at]).ok_or_else(|| Error::NotAKey(x.into()))? + 1;
        if next == DIGITS.len() {
            digits[at] = DIGITS[0];
        } else {
            digits[at] = DIGITS[next];
            carry = false;
        }
    }
    if !carry {
        return Ok(Some(format!(
            "{}{}",
            char::from(head),
            String::from_utf8_lossy(&digits)
        )));
    }
    // The whole part is full, so the head moves on and the part changes length.
    match head {
        b'Z' => Ok(Some(format!("a{}", zero()))),
        b'z' => Ok(None),
        _ => {
            let next = head + 1;
            if next > b'a' {
                digits.push(DIGITS[0]);
            } else {
                digits.pop();
            }
            Ok(Some(format!(
                "{}{}",
                char::from(next),
                String::from_utf8_lossy(&digits)
            )))
        }
    }
}

/// The whole-number part one before `x`, when there is one.
fn decrement(x: &str) -> Result<Option<String>, Error> {
    if x.len() != integer_length(x.as_bytes()[0])? {
        return Err(Error::NotAKey(x.into()));
    }
    let head = x.as_bytes()[0];
    let mut digits: Vec<u8> = x.bytes().skip(1).collect();
    let mut borrow = true;
    for at in (0..digits.len()).rev() {
        if !borrow {
            break;
        }
        match digit(digits[at]).ok_or_else(|| Error::NotAKey(x.into()))? {
            0 => digits[at] = DIGITS[DIGITS.len() - 1],
            held => {
                digits[at] = DIGITS[held - 1];
                borrow = false;
            }
        }
    }
    if !borrow {
        return Ok(Some(format!(
            "{}{}",
            char::from(head),
            String::from_utf8_lossy(&digits)
        )));
    }
    match head {
        b'a' => Ok(Some(format!("Z{}", last_digit()))),
        b'A' => Ok(None),
        _ => {
            let next = head - 1;
            if next < b'Z' {
                digits.push(DIGITS[DIGITS.len() - 1]);
            } else {
                digits.pop();
            }
            Ok(Some(format!(
                "{}{}",
                char::from(next),
                String::from_utf8_lossy(&digits)
            )))
        }
    }
}

/// A key that sorts between `a` and `b`.
///
/// Nothing on either side means the end of the order in that direction, so `between(None, None)` is
/// the first key of an empty drawing.
///
/// # Errors
///
/// If either key is not one, if they are not in order, or if there is no room past the one given.
pub fn between(a: Option<&str>, b: Option<&str>) -> Result<String, Error> {
    if let Some(a) = a {
        validate(a)?;
    }
    if let Some(b) = b {
        validate(b)?;
    }
    match (a, b) {
        (Some(a), Some(b)) if a >= b => Err(Error::OutOfOrder(a.into(), b.into())),
        (None, None) => Ok(format!("a{}", zero())),
        (None, Some(b)) => {
            let whole = integer_part(b)?;
            let fraction = &b[whole.len()..];
            let reserved = format!("A{}", zero().to_string().repeat(26));
            if whole == reserved {
                return Ok(format!("{whole}{}", midpoint("", Some(fraction))?));
            }
            if whole < b {
                return Ok(whole.to_owned());
            }
            decrement(whole)?.ok_or_else(|| Error::NoRoom(b.into()))
        }
        (Some(a), None) => {
            let whole = integer_part(a)?;
            let fraction = &a[whole.len()..];
            match increment(whole)? {
                Some(next) => Ok(next),
                None => Ok(format!("{whole}{}", midpoint(fraction, None)?)),
            }
        }
        (Some(a), Some(b)) => {
            let whole_a = integer_part(a)?;
            let fraction_a = &a[whole_a.len()..];
            let whole_b = integer_part(b)?;
            let fraction_b = &b[whole_b.len()..];
            if whole_a == whole_b {
                return Ok(format!(
                    "{whole_a}{}",
                    midpoint(fraction_a, Some(fraction_b))?
                ));
            }
            let next = increment(whole_a)?.ok_or_else(|| Error::NoRoom(a.into()))?;
            if next.as_str() < b {
                return Ok(next);
            }
            Ok(format!("{whole_a}{}", midpoint(fraction_a, None)?))
        }
    }
}

/// `count` keys in order between `a` and `b`.
///
/// # Errors
///
/// The same reasons as [`between`].
pub fn n_between(a: Option<&str>, b: Option<&str>, count: usize) -> Result<Vec<String>, Error> {
    match count {
        0 => Ok(Vec::new()),
        1 => Ok(vec![between(a, b)?]),
        _ => match (a, b) {
            // Walking out from one end: each key is made past the one before it.
            (_, None) => {
                let mut held = between(a, None)?;
                let mut out = vec![held.clone()];
                for _ in 0..count - 1 {
                    held = between(Some(&held), None)?;
                    out.push(held.clone());
                }
                Ok(out)
            }
            (None, _) => {
                let mut held = between(None, b)?;
                let mut out = vec![held.clone()];
                for _ in 0..count - 1 {
                    held = between(None, Some(&held))?;
                    out.push(held.clone());
                }
                out.reverse();
                Ok(out)
            }
            // Between two keys, the middle is made first and each half filled in, so the keys stay
            // short rather than growing one digit per element.
            (Some(a), Some(b)) => {
                let half = count / 2;
                let middle = between(Some(a), Some(b))?;
                let mut out = n_between(Some(a), Some(&middle), half)?;
                out.push(middle.clone());
                out.extend(n_between(Some(&middle), Some(b), count - half - 1)?);
                Ok(out)
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_key_of_an_empty_drawing_is_a0() {
        assert_eq!(between(None, None).expect("a key"), "a0");
    }

    #[test]
    fn a_key_past_one_sorts_after_it() {
        let first = between(None, None).expect("a key");
        let second = between(Some(&first), None).expect("a key");
        assert!(second > first);
        assert_eq!(second, "a1");
    }

    #[test]
    fn a_key_before_one_sorts_before_it() {
        let first = between(None, None).expect("a key");
        let before = between(None, Some(&first)).expect("a key");
        assert!(before < first);
    }

    #[test]
    fn a_key_between_two_sorts_between_them() {
        let held = between(Some("a0"), Some("a1")).expect("a key");
        assert!(held.as_str() > "a0" && held.as_str() < "a1");
        assert!(is_valid(&held));
    }

    #[test]
    fn many_keys_come_back_in_order() {
        let keys = n_between(Some("a0"), Some("a1"), 20).expect("keys");
        assert_eq!(keys.len(), 20);
        for pair in keys.windows(2) {
            assert!(pair[0] < pair[1], "{} is not before {}", pair[0], pair[1]);
            assert!(is_valid(&pair[0]));
        }
        assert!(keys[0].as_str() > "a0");
        assert!(keys[19].as_str() < "a1");
    }

    #[test]
    fn keys_walking_off_one_end_stay_in_order() {
        for (a, b) in [(Some("a0"), None), (None, Some("a0"))] {
            let keys = n_between(a, b, 12).expect("keys");
            assert_eq!(keys.len(), 12);
            for pair in keys.windows(2) {
                assert!(pair[0] < pair[1]);
            }
        }
    }

    #[test]
    fn many_keys_between_stay_short() {
        let keys = n_between(Some("a0"), Some("a1"), 100).expect("keys");
        let longest = keys.iter().map(String::len).max().expect("some keys");
        assert!(longest <= 6, "the longest key is {longest} letters");
    }

    #[test]
    fn a_key_that_is_not_one_is_refused() {
        assert!(!is_valid(""));
        assert!(!is_valid("0"));
        assert!(!is_valid("a"));
        assert!(!is_valid("a00"), "a trailing zero is not allowed");
        assert!(!is_valid("a!"));
        assert!(is_valid("a0"));
        assert!(is_valid("Zz"));
    }

    #[test]
    fn two_keys_the_wrong_way_round_are_refused() {
        assert!(matches!(
            between(Some("a1"), Some("a0")),
            Err(Error::OutOfOrder(..))
        ));
    }

    #[test]
    fn no_keys_is_no_work() {
        assert!(n_between(None, None, 0).expect("no keys").is_empty());
    }
}
