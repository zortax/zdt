//! Keeping the keys in step with the order.
//!
//! The array is what a drawing is painted in; the keys are only how two people who reordered it at
//! the same time agree on what it became. So after anything that changes the order, the keys that
//! no longer sort with their neighbours are made again, and nothing else is touched.

use super::{Error, is_valid, n_between};

/// The keys, with every invalid one replaced.
///
/// A key is invalid when it is missing, is not a key at all, or does not sort between the keys
/// either side of it. Each run of invalid keys is filled in from the good keys around it, so a
/// drawing whose keys were already right keeps every one of them.
///
/// # Errors
///
/// If a run cannot be filled, which needs a drawing with more elements between two keys than there
/// is room for.
pub fn sync_invalid(keys: &[Option<String>]) -> Result<Vec<String>, Error> {
    let mut out: Vec<Option<String>> = keys.to_vec();
    let mut at = 0;
    while at < out.len() {
        if valid_here(&out, at) {
            at += 1;
            continue;
        }
        // Everything from here up to the next good key is made again in one go.
        let mut end = at + 1;
        while end < out.len() && !valid_here(&out, end) {
            end += 1;
        }
        let before = at.checked_sub(1).and_then(|before| out[before].clone());
        let after = out.get(end).and_then(Clone::clone);
        let made = n_between(before.as_deref(), after.as_deref(), end - at)?;
        for (slot, key) in out[at..end].iter_mut().zip(made) {
            *slot = Some(key);
        }
        at = end;
    }
    Ok(out.into_iter().map(|key| key.unwrap_or_default()).collect())
}

/// Whether the key at `at` is one, and sorts after the one before it.
fn valid_here(keys: &[Option<String>], at: usize) -> bool {
    let Some(Some(key)) = keys.get(at) else {
        return false;
    };
    if !is_valid(key) {
        return false;
    }
    match at.checked_sub(1).and_then(|before| keys[before].as_ref()) {
        Some(before) => before < key,
        None => true,
    }
}

/// The keys, with only the ones at `moved` made again.
///
/// What did not move keeps its key, which is what makes a reorder a small change rather than a
/// rewrite of every element.
///
/// # Errors
///
/// The same reasons as [`sync_invalid`].
pub fn sync_moved(keys: &[Option<String>], moved: &[usize]) -> Result<Vec<String>, Error> {
    let mut out: Vec<Option<String>> = keys.to_vec();
    for at in moved {
        if let Some(slot) = out.get_mut(*at) {
            *slot = None;
        }
    }
    sync_invalid(&out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(held: &[Option<&str>]) -> Vec<Option<String>> {
        held.iter().map(|key| key.map(str::to_owned)).collect()
    }

    #[test]
    fn keys_that_already_sort_are_left_alone() {
        let held = keys(&[Some("a0"), Some("a1"), Some("a2")]);
        let out = sync_invalid(&held).expect("keys");
        assert_eq!(out, ["a0", "a1", "a2"]);
    }

    #[test]
    fn a_missing_key_is_made_between_its_neighbours() {
        let held = keys(&[Some("a0"), None, Some("a2")]);
        let out = sync_invalid(&held).expect("keys");
        assert_eq!(out[0], "a0");
        assert_eq!(out[2], "a2");
        assert!(out[1].as_str() > "a0" && out[1].as_str() < "a2");
    }

    #[test]
    fn a_drawing_with_no_keys_at_all_gets_them() {
        let held = keys(&[None, None, None]);
        let out = sync_invalid(&held).expect("keys");
        assert_eq!(out.len(), 3);
        for pair in out.windows(2) {
            assert!(pair[0] < pair[1]);
        }
    }

    #[test]
    fn a_key_out_of_order_is_made_again() {
        let held = keys(&[Some("a2"), Some("a1"), Some("a3")]);
        let out = sync_invalid(&held).expect("keys");
        for pair in out.windows(2) {
            assert!(pair[0] < pair[1], "{} is not before {}", pair[0], pair[1]);
        }
        assert_eq!(out[0], "a2", "the first was already fine");
    }

    #[test]
    fn only_what_moved_is_made_again() {
        let held = keys(&[Some("a0"), Some("aV"), Some("a2")]);
        let out = sync_moved(&held, &[1]).expect("keys");
        assert_eq!(out[0], "a0", "what did not move kept its key");
        assert_eq!(out[2], "a2");
        assert_ne!(out[1], "aV", "what moved was given a new one");
        assert!(out[1].as_str() > "a0" && out[1].as_str() < "a2");
    }

    #[test]
    fn a_key_that_is_not_one_is_replaced() {
        let held = keys(&[Some("a0"), Some("nonsense!"), Some("a2")]);
        let out = sync_invalid(&held).expect("keys");
        assert!(is_valid(&out[1]));
    }

    #[test]
    fn nothing_stays_nothing() {
        assert!(sync_invalid(&[]).expect("no keys").is_empty());
    }
}
