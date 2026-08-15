//! Jumping to a place you can see.
//!
//! leap.nvim's motion: type `s`, then the two characters you are aiming at, and a label appears on
//! every place those two characters occur. Type the label and the caret is there.
//!
//! Everything here is pure over a `&Rope` and a byte range, so the whole thing can be tested by
//! writing down some text and asserting where the labels land.
//!
//! # Why two characters
//!
//! One is not enough — a screen of code has fifty `e`s — and three is more than the eye needs.
//! Two narrows a screenful to a handful, which is few enough that the labels can be single keys
//! from a home-row alphabet rather than sequences.

use ropey::Rope;

/// Which way a leap looks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    /// Forward from the caret — `s`.
    Forward,
    /// Backward from the caret — `S`.
    Backward,
    /// Both, nearest first — `gs`, and what a window-wide leap comes to.
    Both,
}

/// One place a leap could go.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Landing {
    /// Where it is, as a byte offset.
    pub at: usize,
    /// The key that takes you there.
    pub label: char,
}

/// The keys labels are drawn from, in the order they are handed out.
///
/// leap.nvim's own, which is the home row and its neighbours: the earliest labels are the ones
/// the fingers are already on, and the order is chosen so that no label is a prefix of a reach
/// across the keyboard.
pub const ALPHABET: &str = "sfnjklhodweimbuyvrgtaqpcxz";

/// Every place `pair` occurs in `window`, labelled, nearest to `caret` first.
///
/// `window` is the visible byte range: a leap offers what can be seen and nothing else, which is
/// what keeps the labels few and the choice honest.
///
/// Matching is case-insensitive when `pair` is all lower case, on the same reasoning as a search:
/// somebody who typed no capitals did not mean to be fussy.
#[must_use]
pub fn landings(
    rope: &Rope,
    window: std::ops::Range<usize>,
    caret: usize,
    pair: &str,
    direction: Direction,
    alphabet: &str,
) -> Vec<Landing> {
    let mut found = matches_in(rope, window, caret, pair, direction);

    // Nearest first, so the earliest labels — the ones under the fingers — go to the places the
    // eye is most likely already on.
    found.sort_by_key(|at| at.abs_diff(caret));

    let labels: Vec<char> = alphabet.chars().collect();
    found
        .into_iter()
        .zip(labels)
        .map(|(at, label)| Landing { at, label })
        .collect()
}

/// Where `pair` occurs, unlabelled and in document order.
#[must_use]
pub fn matches_in(
    rope: &Rope,
    window: std::ops::Range<usize>,
    caret: usize,
    pair: &str,
    direction: Direction,
) -> Vec<usize> {
    if pair.is_empty() {
        return Vec::new();
    }
    let start = window.start.min(rope.len_bytes());
    let end = window.end.min(rope.len_bytes());
    if start >= end {
        return Vec::new();
    }

    let insensitive = !pair.chars().any(char::is_uppercase);
    let text = rope.byte_slice(start..end).to_string();
    let haystack = if insensitive {
        text.to_lowercase()
    } else {
        text.clone()
    };
    // Lowercasing can change a string's length — `İ` becomes two characters — which would put
    // every offset after it wrong. When it does, match case-sensitively instead: a wrong jump is
    // worse than a fussy one.
    let haystack = if haystack.len() == text.len() {
        haystack
    } else {
        text.clone()
    };
    let needle = if insensitive && haystack.len() == text.len() {
        pair.to_lowercase()
    } else {
        pair.to_owned()
    };

    let mut found = Vec::new();
    let mut from = 0;
    while let Some(offset) = haystack[from..].find(&needle) {
        let at = start + from + offset;
        // Only where a character begins: a match inside a multi-byte character is not a place a
        // caret can go.
        if rope.try_byte_to_char(at).is_ok() {
            let wanted = match direction {
                Direction::Forward => at > caret,
                Direction::Backward => at < caret,
                Direction::Both => at != caret,
            };
            if wanted {
                found.push(at);
            }
        }
        // Overlapping matches count: `aaa` has two places `aa` begins, and a leap that offered
        // one of them would be a leap that could not reach the other. So the search resumes one
        // character on rather than past the whole match — one *character*, because slicing a
        // string in the middle of one is a panic.
        from += offset + 1;
        while from < haystack.len() && !haystack.is_char_boundary(from) {
            from += 1;
        }
        if from >= haystack.len() {
            break;
        }
    }
    found
}

/// What a leap is waiting for.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Nothing typed yet.
    First,
    /// One character typed; the second narrows it.
    Second(char),
    /// Both typed, and these are the places it could go.
    Choosing(Vec<Landing>),
}

/// A leap in progress.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Leap {
    /// Which way it looks.
    pub direction: Direction,
    /// What it is waiting for.
    pub phase: Phase,
    /// What has been typed so far.
    pub typed: String,
}

impl Leap {
    /// A leap that has just started.
    #[must_use]
    pub fn new(direction: Direction) -> Self {
        Self {
            direction,
            phase: Phase::First,
            typed: String::new(),
        }
    }

    /// The places it could go, when it has any.
    #[must_use]
    pub fn landings(&self) -> &[Landing] {
        match &self.phase {
            Phase::Choosing(landings) => landings,
            _ => &[],
        }
    }

    /// Where the label `key` goes, when it is one of them.
    #[must_use]
    pub fn choose(&self, key: char) -> Option<usize> {
        self.landings()
            .iter()
            .find(|landing| landing.label == key)
            .map(|landing| landing.at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rope(text: &str) -> Rope {
        Rope::from_str(text)
    }

    #[test]
    fn a_pair_is_found_where_it_occurs() {
        let text = rope("the cat sat on the mat");
        let found = matches_in(&text, 0..text.len_bytes(), 0, "at", Direction::Forward);
        assert_eq!(found, vec![5, 9, 20], "cat, sat, mat");
    }

    #[test]
    fn only_what_can_be_seen() {
        let text = rope("at one end\nand at the other");
        // A window over the second line only.
        let found = matches_in(&text, 11..text.len_bytes(), 0, "at", Direction::Forward);
        assert_eq!(found.len(), 1);
        assert!(found[0] > 11);
    }

    #[test]
    fn forward_and_backward_look_the_way_they_say() {
        let text = rope("at .. at .. at");
        let window = 0..text.len_bytes();

        assert_eq!(
            matches_in(&text, window.clone(), 6, "at", Direction::Forward),
            vec![12]
        );
        assert_eq!(
            matches_in(&text, window.clone(), 6, "at", Direction::Backward),
            vec![0]
        );
        assert_eq!(
            matches_in(&text, window, 6, "at", Direction::Both),
            vec![0, 12],
            "both ways, and the place the caret is already on is not offered"
        );
    }

    #[test]
    fn the_caret_is_not_a_place_to_leap_to() {
        let text = rope("at .. at");
        let found = matches_in(&text, 0..text.len_bytes(), 0, "at", Direction::Both);
        assert_eq!(found, vec![6], "the one under the caret is left out");
    }

    #[test]
    fn overlapping_matches_are_both_offered() {
        let text = rope("aaa");
        let found = matches_in(&text, 0..3, 3, "aa", Direction::Backward);
        assert_eq!(found, vec![0, 1]);
    }

    #[test]
    fn lower_case_is_not_fussy_and_a_capital_is() {
        let text = rope("At and at");
        let window = 0..text.len_bytes();

        assert_eq!(
            matches_in(&text, window.clone(), 100, "at", Direction::Backward).len(),
            2,
            "typed in lower case, both are found"
        );
        assert_eq!(
            matches_in(&text, window, 100, "At", Direction::Backward),
            vec![0],
            "with a capital, only the one that has it"
        );
    }

    #[test]
    fn the_nearest_place_gets_the_first_label() {
        let text = rope("at ......... at .. at");
        let found = landings(
            &text,
            0..text.len_bytes(),
            19,
            "at",
            Direction::Both,
            ALPHABET,
        );

        // 19 is the last `at`; the nearest other is at 13, then 0.
        assert_eq!(found[0].at, 13);
        assert_eq!(found[0].label, 's');
        assert_eq!(found[1].at, 0);
        assert_eq!(found[1].label, 'f');
    }

    #[test]
    fn there_are_never_more_labels_than_letters() {
        let text = rope(&"at ".repeat(200));
        let found = landings(
            &text,
            0..text.len_bytes(),
            0,
            "at",
            Direction::Forward,
            ALPHABET,
        );
        assert_eq!(found.len(), ALPHABET.chars().count());
    }

    #[test]
    fn a_label_says_where_it_goes() {
        let text = rope("at .. at");
        let landings = landings(&text, 0..8, 0, "at", Direction::Forward, ALPHABET);
        let leap = Leap {
            direction: Direction::Forward,
            phase: Phase::Choosing(landings),
            typed: "at".to_owned(),
        };

        assert_eq!(leap.choose('s'), Some(6));
        assert_eq!(leap.choose('q'), None, "a key that labels nothing");
    }

    #[test]
    fn an_empty_pair_finds_nothing() {
        let text = rope("at");
        assert!(matches_in(&text, 0..2, 0, "", Direction::Forward).is_empty());
    }

    #[test]
    fn a_window_outside_the_text_finds_nothing() {
        let text = rope("at");
        assert!(matches_in(&text, 100..200, 0, "at", Direction::Forward).is_empty());
    }

    #[test]
    fn a_match_inside_a_character_is_not_a_place() {
        // Two characters that share no byte with an ASCII pair: nothing should be found in the
        // middle of one.
        let text = rope("→→");
        let found = matches_in(&text, 0..text.len_bytes(), 0, "→→", Direction::Forward);
        assert!(found.is_empty(), "the only match is under the caret");
        let found = matches_in(&text, 0..text.len_bytes(), 9, "→→", Direction::Backward);
        assert_eq!(found, vec![0], "and it begins where a character does");
    }
}
