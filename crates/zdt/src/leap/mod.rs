//! Leap, as the interface sees it.
//!
//! The finding and the labelling are [`zdt_vim::leap`]'s and are pure. This is the three
//! keystrokes around them: what `s` starts, what the two characters narrow, and what the label
//! key does.
//!
//! # Where the keys come from
//!
//! From here, and never the keymap. Once a leap has started, every key is either a character it
//! is aiming at or a label. A keymap that answered any of them would make some letters
//! unreachable. The modal layer asks here first, and falls through only when nothing is in
//! progress.
//!
//! # Operator-pending
//!
//! `ds{ab}` deletes to where the leap lands, so a leap started while an operator is waiting comes
//! back as a motion. That is why choosing answers the byte and moves nothing. The caller knows
//! whether an operator is pending, and this does not.

pub mod view;

use std::cell::RefCell;
use std::rc::Rc;

use zdt_vim::leap::{ALPHABET, Direction, Landing, Leap, Phase};
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

/// A leap in progress, and the labels the interface draws.
#[derive(Clone)]
pub struct Leaping {
    inner: Rc<Inner>,
}

struct Inner {
    /// What is in progress, when anything is.
    state: RefCell<Option<Leap>>,
    /// The labels, for drawing. A signal because the overlay follows them.
    labels: RwSignal<Vec<Landing>, LocalStorage>,
    /// What has been typed, echoed in the status line the way a pending key sequence is.
    typed: RwSignal<String, LocalStorage>,
    /// The keys labels are drawn from.
    alphabet: RefCell<String>,
}

/// What taking a key did.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Took {
    /// Nothing was in progress; the key belongs to whoever asked.
    NotMine,
    /// It was used, and there is more to type.
    Waiting,
    /// A label was chosen: put the caret here.
    Landed(usize),
    /// It ended without going anywhere.
    Cancelled,
}

impl Leaping {
    /// Nothing in progress.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Rc::new(Inner {
                state: RefCell::new(None),
                labels: RwSignal::new_local(Vec::new()),
                typed: RwSignal::new_local(String::new()),
                alphabet: RefCell::new(ALPHABET.to_owned()),
            }),
        }
    }

    /// Changes the keys labels are drawn from.
    pub fn set_alphabet(&self, alphabet: &str) {
        let alphabet = if alphabet.is_empty() {
            ALPHABET
        } else {
            alphabet
        };
        *self.inner.alphabet.borrow_mut() = alphabet.to_owned();
    }

    /// The labels to draw. Tracked.
    #[must_use]
    pub fn labels(&self) -> Vec<Landing> {
        self.inner.labels.get()
    }

    /// Whether one is in progress. Tracked.
    #[must_use]
    pub fn is_active(&self) -> bool {
        !self.inner.typed.with(String::is_empty) || !self.inner.labels.with(Vec::is_empty)
    }

    /// Whether one is in progress, without subscribing.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.inner.state.borrow().is_some()
    }

    /// What has been typed toward one. Tracked.
    #[must_use]
    pub fn typed(&self) -> String {
        self.inner.typed.get()
    }

    /// Starts one looking `direction`.
    pub fn start(&self, direction: Direction) {
        *self.inner.state.borrow_mut() = Some(Leap::new(direction));
        self.inner.labels.set(Vec::new());
        // A space, so the status line shows that something is waiting even before a character has
        // been typed. An empty echo would look like nothing had happened.
        self.inner.typed.set(" ".to_owned());
    }

    /// Ends it, drawing nothing.
    pub fn stop(&self) {
        if self.inner.state.borrow().is_some() {
            *self.inner.state.borrow_mut() = None;
        }
        if !self.inner.labels.with_untracked(Vec::is_empty) {
            self.inner.labels.set(Vec::new());
        }
        if !self.inner.typed.with_untracked(String::is_empty) {
            self.inner.typed.set(String::new());
        }
    }

    /// Takes one key.
    ///
    /// `find` is asked for the places a pair occurs; it is a closure because the places depend on
    /// what is on screen, which is the view's business and not this module's.
    pub fn key(
        &self,
        character: Option<char>,
        find: impl FnOnce(&str, Direction, &str) -> Vec<Landing>,
    ) -> Took {
        let Some(leap) = self.inner.state.borrow().clone() else {
            return Took::NotMine;
        };

        // Anything other than a character ends it: an arrow, a modifier chord, `<Esc>`. A leap
        // is three keystrokes, and anything else means the hands have moved on.
        let Some(character) = character else {
            self.stop();
            return Took::Cancelled;
        };

        match leap.phase {
            Phase::First => {
                let mut next = leap;
                next.typed.push(character);
                next.phase = Phase::Second(character);
                self.inner.typed.set(next.typed.clone());
                *self.inner.state.borrow_mut() = Some(next);
                Took::Waiting
            }
            Phase::Second(_) => {
                let mut next = leap;
                next.typed.push(character);
                let alphabet = self.inner.alphabet.borrow().clone();
                let landings = find(&next.typed, next.direction, &alphabet);

                if landings.is_empty() {
                    self.stop();
                    return Took::Cancelled;
                }
                // One place to go is not a choice. Going straight there saves a keystroke, which
                // over a day is most of what this motion is for.
                if landings.len() == 1 {
                    let at = landings[0].at;
                    self.stop();
                    return Took::Landed(at);
                }

                self.inner.typed.set(next.typed.clone());
                self.inner.labels.set(landings.clone());
                next.phase = Phase::Choosing(landings);
                *self.inner.state.borrow_mut() = Some(next);
                Took::Waiting
            }
            Phase::Choosing(_) => {
                let chosen = leap.choose(character);
                self.stop();
                match chosen {
                    Some(at) => Took::Landed(at),
                    // A key that labels nothing ends it. A leap that stayed open through a
                    // mistyped label would eat the keys after it too.
                    None => Took::Cancelled,
                }
            }
        }
    }
}

impl Default for Leaping {
    fn default() -> Self {
        Self::new()
    }
}

/// Puts it where every component can find it.
pub fn provide(leaping: Leaping) {
    zgui::reactive::provide_local_context(leaping);
}

/// It, from inside a component.
///
/// # Panics
///
/// If none was provided above this component, which is a wiring mistake.
#[must_use]
pub fn use_leaping() -> Leaping {
    zgui::reactive::use_local_context::<Leaping>().expect("a leap layer is provided at the root")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two places, labelled `s` and `f`.
    fn two(_pair: &str, _direction: Direction, alphabet: &str) -> Vec<Landing> {
        let mut labels = alphabet.chars();
        vec![
            Landing {
                at: 10,
                label: labels.next().unwrap_or('s'),
            },
            Landing {
                at: 20,
                label: labels.next().unwrap_or('f'),
            },
        ]
    }

    /// One place.
    fn one(_pair: &str, _direction: Direction, alphabet: &str) -> Vec<Landing> {
        vec![Landing {
            at: 7,
            label: alphabet.chars().next().unwrap_or('s'),
        }]
    }

    /// None.
    fn none(_pair: &str, _direction: Direction, _alphabet: &str) -> Vec<Landing> {
        Vec::new()
    }

    #[test]
    fn a_key_before_it_starts_is_not_its_business() {
        let leaping = Leaping::new();
        assert_eq!(leaping.key(Some('a'), two), Took::NotMine);
    }

    #[test]
    fn two_characters_then_a_label() {
        let leaping = Leaping::new();
        leaping.start(Direction::Forward);

        assert_eq!(leaping.key(Some('a'), none), Took::Waiting);
        assert_eq!(leaping.key(Some('t'), two), Took::Waiting);
        assert_eq!(leaping.labels().len(), 2, "and the labels are drawn");

        assert_eq!(leaping.key(Some('f'), none), Took::Landed(20));
        assert!(leaping.labels().is_empty(), "which are cleared after");
        assert!(!leaping.is_running());
    }

    #[test]
    fn one_place_needs_no_label() {
        let leaping = Leaping::new();
        leaping.start(Direction::Forward);

        leaping.key(Some('a'), none);
        assert_eq!(
            leaping.key(Some('t'), one),
            Took::Landed(7),
            "a choice of one is not a choice"
        );
    }

    #[test]
    fn nowhere_to_go_ends_it() {
        let leaping = Leaping::new();
        leaping.start(Direction::Forward);

        leaping.key(Some('z'), none);
        assert_eq!(leaping.key(Some('z'), none), Took::Cancelled);
        assert!(!leaping.is_running());
    }

    #[test]
    fn a_key_that_is_not_a_character_ends_it() {
        let leaping = Leaping::new();
        leaping.start(Direction::Forward);

        assert_eq!(leaping.key(None, none), Took::Cancelled);
        assert!(!leaping.is_running());
    }

    #[test]
    fn a_label_nothing_carries_ends_it_rather_than_waiting() {
        let leaping = Leaping::new();
        leaping.start(Direction::Forward);

        leaping.key(Some('a'), none);
        leaping.key(Some('t'), two);
        assert_eq!(leaping.key(Some('q'), none), Took::Cancelled);
        assert!(!leaping.is_running());
    }

    #[test]
    fn the_alphabet_can_be_changed() {
        let leaping = Leaping::new();
        leaping.set_alphabet("xy");
        leaping.start(Direction::Forward);

        leaping.key(Some('a'), none);
        leaping.key(Some('t'), two);
        assert_eq!(
            leaping.labels().first().map(|landing| landing.label),
            Some('x')
        );
        assert_eq!(leaping.key(Some('y'), none), Took::Landed(20));
    }

    #[test]
    fn an_empty_alphabet_falls_back_to_the_shipped_one() {
        let leaping = Leaping::new();
        leaping.set_alphabet("");
        leaping.start(Direction::Forward);

        leaping.key(Some('a'), none);
        leaping.key(Some('t'), two);
        assert_eq!(
            leaping.labels().first().map(|landing| landing.label),
            Some('s')
        );
    }
}
