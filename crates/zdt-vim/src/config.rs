//! Reading a keymap out of a file.
//!
//! ```toml
//! [groups]
//! "<Leader>f" = "Find"
//!
//! [[map]]
//! mode = ["n", "x"]
//! keys = "<Leader>ff"
//! action = "picker.files"
//! desc = "Find files"
//!
//! [[map]]
//! mode = ["n", "o", "x"]
//! keys = "w"
//! action = "motion.word_forward"
//! args = { big = false }
//! ```
//!
//! A row can carry several actions with `actions = [...]`, and can take a binding away with
//! `action = false` — which is how a person removes one of the defaults rather than having to
//! shadow it with something that does nothing.
//!
//! Files are merged in the order they are given, so the shipped defaults are read first and a
//! person's own file is an override of them.

use serde::Deserialize;

use crate::action::{Action, Args};
use crate::chord::Chord;
use crate::keymap::{Binding, Keymap};
use crate::mode::ModeSet;
use crate::notation::{Leaders, parse};

/// What went wrong reading a keymap.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum KeymapError {
    /// The file is not the shape a keymap is.
    #[error("{0}")]
    Malformed(String),
    /// A row's key sequence could not be read.
    #[error("{keys:?}: {reason}")]
    BadKeys {
        /// What the row said.
        keys: String,
        /// Why it could not be read.
        reason: String,
    },
    /// A row named a mode letter that stands for nothing.
    #[error("{keys:?}: `{letter}` is not a mode")]
    BadMode {
        /// What the row said.
        keys: String,
        /// The letter it used.
        letter: String,
    },
    /// A row said nothing about what it does.
    #[error("{keys:?}: no action")]
    NoAction {
        /// What the row said.
        keys: String,
    },
}

/// The whole file.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    /// What a prefix is called, which is what which-key shows above it.
    #[serde(default)]
    groups: std::collections::BTreeMap<String, String>,
    /// The bindings.
    #[serde(default, rename = "map")]
    rows: Vec<Row>,
}

/// One binding, as it is written.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    /// Which modes it applies in. Normal when it does not say.
    #[serde(default)]
    mode: Vec<String>,
    /// The keys.
    keys: String,
    /// What it does, or `false` to take a binding away.
    #[serde(default)]
    action: Option<toml::Value>,
    /// Several things, in order.
    #[serde(default)]
    actions: Vec<ActionRow>,
    /// What its one action was given.
    #[serde(default)]
    args: Option<toml::Table>,
    /// What which-key calls it.
    #[serde(default)]
    desc: Option<String>,
}

/// One of several actions in a row.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActionRow {
    /// What it is called.
    action: String,
    /// What it was given.
    #[serde(default)]
    args: Option<toml::Table>,
}

/// Reads `text` into `map`, on top of whatever is already there.
///
/// Bindings replace what was bound to the same keys in the same mode, so a person's file layered
/// over the defaults is an override of them. Every row is read even when one of them is wrong;
/// the mistakes come back together so that a typo in the middle of a file does not hide what is
/// after it.
pub fn merge(map: &mut Keymap, text: &str, leaders: Leaders) -> Result<(), Vec<KeymapError>> {
    let file: File = match toml::from_str(text) {
        Ok(file) => file,
        Err(error) => return Err(vec![KeymapError::Malformed(error.to_string())]),
    };

    let mut problems = Vec::new();

    for (keys, label) in &file.groups {
        match parse(keys, leaders) {
            // A group is a label for a prefix in every mode it might be typed in: which-key
            // shows the same name for `<Leader>f` whether the person is in normal or visual.
            Ok(chords) => map.name_group(ModeSet::ALL, &chords, label.clone()),
            Err(error) => problems.push(KeymapError::BadKeys {
                keys: keys.clone(),
                reason: error.to_string(),
            }),
        }
    }

    for row in &file.rows {
        if let Err(problem) = apply(map, row, leaders) {
            problems.push(problem);
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// One row, applied.
fn apply(map: &mut Keymap, row: &Row, leaders: Leaders) -> Result<(), KeymapError> {
    let chords: Vec<Chord> = parse(&row.keys, leaders).map_err(|error| KeymapError::BadKeys {
        keys: row.keys.clone(),
        reason: error.to_string(),
    })?;

    let modes = if row.mode.is_empty() {
        ModeSet::from_letter("n").expect("n is a mode letter")
    } else {
        let mut modes = ModeSet::NONE;
        for letter in &row.mode {
            let one = ModeSet::from_letter(letter).ok_or_else(|| KeymapError::BadMode {
                keys: row.keys.clone(),
                letter: letter.clone(),
            })?;
            modes = modes.with(one);
        }
        modes
    };

    // `action = false` takes the binding away.
    if row.action.as_ref().and_then(toml::Value::as_bool) == Some(false) {
        map.unbind(modes, &chords);
        return Ok(());
    }

    let mut actions = Vec::new();
    if let Some(name) = row.action.as_ref().and_then(|value| value.as_str()) {
        actions.push(Action::with(
            name,
            row.args.clone().map(Args::new).unwrap_or_default(),
        ));
    }
    for extra in &row.actions {
        actions.push(Action::with(
            extra.action.clone(),
            extra.args.clone().map(Args::new).unwrap_or_default(),
        ));
    }

    if actions.is_empty() {
        return Err(KeymapError::NoAction {
            keys: row.keys.clone(),
        });
    }

    let description = row.desc.clone().unwrap_or_else(|| {
        // Something rather than nothing: a binding with no description still has to be findable
        // in which-key and in the keymap picker.
        actions[0].leaf().replace('_', " ")
    });

    map.bind(
        modes,
        &chords,
        Binding {
            actions,
            description,
        },
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::merge;
    use crate::keymap::{Keymap, Resolution};
    use crate::mode::Mode;
    use crate::notation::{Leaders, parse};

    fn keys(sequence: &str) -> Vec<crate::chord::Chord> {
        parse(sequence, Leaders::default()).expect("the sequence reads")
    }

    fn read(text: &str) -> Keymap {
        let mut map = Keymap::new();
        merge(&mut map, text, Leaders::default()).expect("the keymap reads");
        map
    }

    #[test]
    fn a_row_binds_what_it_says() {
        let map = read(
            r#"
            [[map]]
            mode = ["n"]
            keys = "<Leader>ff"
            action = "picker.files"
            desc = "Find files"
            "#,
        );
        match map.resolve(Mode::Normal, &keys("<Leader>ff")) {
            Resolution::Run(binding) => {
                assert_eq!(binding.actions[0].name, "picker.files");
                assert_eq!(binding.description, "Find files");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_row_with_no_mode_is_normal() {
        let map = read(
            r#"
            [[map]]
            keys = "gd"
            action = "lsp.definition"
            "#,
        );
        assert!(matches!(
            map.resolve(Mode::Normal, &keys("gd")),
            Resolution::Run(_)
        ));
        assert_eq!(map.resolve(Mode::Insert, &keys("gd")), Resolution::None);
    }

    #[test]
    fn arguments_reach_the_action() {
        let map = read(
            r#"
            [[map]]
            mode = ["n", "o", "x"]
            keys = "W"
            action = "motion.word_forward"
            args = { big = true }
            "#,
        );
        match map.resolve(Mode::OperatorPending, &keys("W")) {
            Resolution::Run(binding) => assert!(binding.actions[0].args.flag("big")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_row_can_do_several_things_in_order() {
        let map = read(
            r#"
            [[map]]
            keys = "<Leader>W"
            actions = [
                { action = "buffer.save" },
                { action = "lsp.format" },
            ]
            desc = "Format and save"
            "#,
        );
        match map.resolve(Mode::Normal, &keys("<Leader>W")) {
            Resolution::Run(binding) => {
                let names: Vec<&str> = binding
                    .actions
                    .iter()
                    .map(|action| action.name.as_str())
                    .collect();
                assert_eq!(names, ["buffer.save", "lsp.format"]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_later_file_overrides_an_earlier_one() {
        let mut map = read(
            r#"
            [[map]]
            keys = "<Leader>ff"
            action = "picker.files"
            "#,
        );
        merge(
            &mut map,
            r#"
            [[map]]
            keys = "<Leader>ff"
            action = "picker.git_files"
            "#,
            Leaders::default(),
        )
        .expect("the second file reads");

        match map.resolve(Mode::Normal, &keys("<Leader>ff")) {
            Resolution::Run(binding) => assert_eq!(binding.actions[0].name, "picker.git_files"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_binding_can_be_taken_away() {
        // Which is what a person needs to free a key the defaults took, without having to bind it
        // to something that does nothing.
        let mut map = read(
            r#"
            [[map]]
            keys = "s"
            action = "leap.forward"
            "#,
        );
        merge(
            &mut map,
            r#"
            [[map]]
            keys = "s"
            action = false
            "#,
            Leaders::default(),
        )
        .expect("the second file reads");
        assert_eq!(map.resolve(Mode::Normal, &keys("s")), Resolution::None);
    }

    #[test]
    fn a_group_names_a_prefix() {
        let map = read(
            r#"
            [groups]
            "<Leader>f" = "Find"

            [[map]]
            keys = "<Leader>ff"
            action = "picker.files"
            "#,
        );
        assert_eq!(
            map.group_label(Mode::Normal, &keys("<Leader>f")),
            Some("Find")
        );
    }

    #[test]
    fn a_binding_with_no_description_still_has_one() {
        let map = read(
            r#"
            [[map]]
            keys = "gd"
            action = "lsp.go_to_definition"
            "#,
        );
        match map.resolve(Mode::Normal, &keys("gd")) {
            Resolution::Run(binding) => assert_eq!(binding.description, "go to definition"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn every_mistake_in_a_file_is_reported_rather_than_the_first() {
        // A typo in the middle of a keymap must not hide what is after it.
        let mut map = Keymap::new();
        let problems = merge(
            &mut map,
            r#"
            [[map]]
            keys = "<C-w"
            action = "window.close"

            [[map]]
            keys = "gq"
            mode = ["q"]
            action = "edit.format"

            [[map]]
            keys = "gr"

            [[map]]
            keys = "gd"
            action = "lsp.definition"
            "#,
            Leaders::default(),
        )
        .expect_err("three rows are wrong");

        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(
            matches!(map.resolve(Mode::Normal, &keys("gd")), Resolution::Run(_)),
            "the row after the mistakes was still read"
        );
    }

    #[test]
    fn something_that_is_not_a_keymap_says_so_once() {
        let mut map = Keymap::new();
        let problems = merge(&mut map, "this is not toml at all {{{", Leaders::default())
            .expect_err("it does not read");
        assert_eq!(problems.len(), 1);
    }

    #[test]
    fn an_unknown_field_is_a_mistake_rather_than_a_shrug() {
        // A misspelled `desc` that silently did nothing would be very hard to notice.
        let mut map = Keymap::new();
        assert!(
            merge(
                &mut map,
                "[[map]]\nkeys = \"gd\"\naction = \"a\"\ndescription = \"typo\"",
                Leaders::default(),
            )
            .is_err()
        );
    }
}
