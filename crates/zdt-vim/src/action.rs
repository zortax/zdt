//! What a key sequence does, as data.
//!
//! An action is a name and some arguments, both read straight out of the keymap file. The engine
//! knows what `motion.word_forward` means; the keymap only knows that it is what `w` does. That
//! separation is the whole reason the keymap can be data: adding a binding is a row in a file, and
//! rebinding `w` to something else is changing which name that row holds.
//!
//! Names that begin with `motion.`, `operator.`, `textobject.`, `edit.`, `mode.`, `register.`,
//! `macro.`, `mark.`, `jump.` and `scroll.` belong to this crate. Everything else is the
//! application's, and reaches it unread.

use std::fmt;

/// The arguments an action was given.
///
/// A TOML table, because that is what it was written as. Typed getters rather than a struct per
/// action: an action this crate has never heard of still has to carry its arguments through.
#[derive(Clone, Default, PartialEq)]
pub struct Args(toml::Table);

impl Args {
    /// No arguments.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// The arguments in `table`.
    #[must_use]
    pub fn new(table: toml::Table) -> Self {
        Self(table)
    }

    /// Whether nothing was given.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The table, for an application reading arguments this crate has no opinion about.
    #[must_use]
    pub fn table(&self) -> &toml::Table {
        &self.0
    }

    /// The text `name` was given, when it was given one.
    #[must_use]
    pub fn str(&self, name: &str) -> Option<&str> {
        self.0.get(name)?.as_str()
    }

    /// The flag `name` was given. `false` when it was not, which is what a flag means.
    #[must_use]
    pub fn flag(&self, name: &str) -> bool {
        self.0.get(name).and_then(toml::Value::as_bool) == Some(true)
    }

    /// The number `name` was given, when it was given one that fits.
    #[must_use]
    pub fn number(&self, name: &str) -> Option<i64> {
        self.0.get(name)?.as_integer()
    }

    /// The count `name` was given, at least one.
    ///
    /// Counts in this editor are always one or more — `0` is a motion, not a multiplier — so a
    /// zero or a negative in a keymap is a mistake that reads as one rather than as nothing
    /// happening.
    #[must_use]
    pub fn count(&self, name: &str) -> Option<u32> {
        let value = self.number(name)?;
        u32::try_from(value).ok().filter(|count| *count > 0)
    }

    /// The single character `name` was given, when it was given exactly one.
    #[must_use]
    pub fn char(&self, name: &str) -> Option<char> {
        let text = self.str(name)?;
        let mut characters = text.chars();
        let first = characters.next()?;
        characters.next().is_none().then_some(first)
    }
}

impl fmt::Debug for Args {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            formatter.write_str("Args()")
        } else {
            write!(formatter, "Args({})", self.0)
        }
    }
}

/// One thing a key sequence does.
#[derive(Clone, PartialEq, Debug)]
pub struct Action {
    /// What it is called.
    pub name: String,
    /// What it was given.
    pub args: Args,
}

impl Action {
    /// An action with no arguments.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            args: Args::none(),
        }
    }

    /// An action with arguments.
    #[must_use]
    pub fn with(name: impl Into<String>, args: Args) -> Self {
        Self {
            name: name.into(),
            args,
        }
    }

    /// Whether this action is in the family `prefix`, as `motion.word_forward` is in `motion`.
    #[must_use]
    pub fn is(&self, prefix: &str) -> bool {
        self.name
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('.'))
    }

    /// The part after the family, as `word_forward` is of `motion.word_forward`.
    #[must_use]
    pub fn leaf(&self) -> &str {
        self.name
            .rsplit_once('.')
            .map(|(_, leaf)| leaf)
            .unwrap_or(&self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::{Action, Args};

    fn args(text: &str) -> Args {
        Args::new(text.parse().expect("the table reads"))
    }

    #[test]
    fn an_action_is_a_name_and_what_it_was_given() {
        let action = Action::with("motion.word_forward", args("big = true"));
        assert!(action.is("motion"));
        assert!(!action.is("operator"));
        assert_eq!(action.leaf(), "word_forward");
        assert!(action.args.flag("big"));
    }

    #[test]
    fn a_family_is_matched_at_the_dot() {
        // `motions` must not read as being in the `motion` family.
        let action = Action::new("motions.confusing");
        assert!(!action.is("motion"));
    }

    #[test]
    fn a_missing_argument_is_not_a_wrong_one() {
        let empty = Args::none();
        assert!(empty.is_empty());
        assert_eq!(empty.str("kind"), None);
        assert_eq!(empty.number("count"), None);
        assert!(!empty.flag("around"), "a flag nobody set is off");
    }

    #[test]
    fn a_count_of_zero_is_not_a_count() {
        // Zero is a motion in this editor, never a multiplier, so a keymap that wrote one is
        // asking for something that cannot happen.
        assert_eq!(args("count = 0").count("count"), None);
        assert_eq!(args("count = -3").count("count"), None);
        assert_eq!(args("count = 3").count("count"), Some(3));
    }

    #[test]
    fn a_character_argument_is_exactly_one_character() {
        assert_eq!(args("open = \"(\"").char("open"), Some('('));
        assert_eq!(args("open = \"()\"").char("open"), None);
        assert_eq!(args("open = \"\"").char("open"), None);
    }

    #[test]
    fn an_action_this_crate_never_heard_of_keeps_its_arguments() {
        let action = Action::with("picker.files", args("hidden = true\nroot = \"cwd\""));
        assert!(!action.is("motion"));
        assert!(action.args.flag("hidden"));
        assert_eq!(action.args.str("root"), Some("cwd"));
    }
}
