//! The keymap the editor ships with.
//!
//! The keymap is a file, so a test that reads it is what protects it. A typo in a key sequence, a
//! mode letter that stands for nothing, or a row that says nothing about what it does would
//! otherwise reach a person's hands as a binding that quietly is not there.

use zdt_vim::config::merge;
use zdt_vim::keymap::{Keymap, Resolution};
use zdt_vim::mode::Mode;
use zdt_vim::notation::{Leaders, parse};

/// The shipped keymap, as the editor reads it.
const DEFAULTS: &str = include_str!("../../../assets/keymap.toml");

fn defaults() -> Keymap {
    let mut map = Keymap::new();
    match merge(&mut map, DEFAULTS, Leaders::default()) {
        Ok(()) => map,
        Err(problems) => panic!("the shipped keymap does not read:\n{problems:#?}"),
    }
}

fn keys(sequence: &str) -> Vec<zdt_vim::Chord> {
    parse(sequence, Leaders::default()).expect("the sequence reads")
}

/// What `sequence` runs in `mode`, or nothing.
fn action(map: &Keymap, mode: Mode, sequence: &str) -> Option<String> {
    match map.resolve(mode, &keys(sequence)) {
        Resolution::Run(binding) => Some(binding.actions[0].name.clone()),
        _ => None,
    }
}

#[test]
fn the_shipped_keymap_reads() {
    let map = defaults();
    assert!(
        map.bindings(Mode::Normal).len() > 100,
        "a keymap this small is one that failed to read"
    );
}

#[test]
fn the_vim_grammar_is_in_the_file_rather_than_in_the_code() {
    // The point of a data-driven keymap: `w` and `d` are rows, so they can be moved.
    let map = defaults();
    assert_eq!(
        action(&map, Mode::Normal, "w"),
        Some("motion.word_forward".to_owned())
    );
    assert_eq!(
        action(&map, Mode::Normal, "d"),
        Some("operator.delete".to_owned())
    );
    assert_eq!(
        action(&map, Mode::OperatorPending, "iw"),
        Some("textobject.word".to_owned())
    );
}

#[test]
fn the_leader_map_is_the_one_the_hands_already_know() {
    let map = defaults();
    for (sequence, name) in [
        ("<Leader>ff", "picker.files"),
        ("<Leader>fw", "picker.grep"),
        ("<Leader>e", "tree.toggle"),
        ("<Leader>w", "buffer.save"),
        ("<Leader>c", "buffer.close"),
        ("<Leader>tf", "terminal.toggle"),
        ("<Leader>la", "lsp.code_action"),
        ("<Leader>lr", "lsp.rename"),
        ("<Leader>Sf", "session.pick"),
    ] {
        assert_eq!(
            action(&map, Mode::Normal, sequence),
            Some(name.to_owned()),
            "{sequence}"
        );
    }
}

#[test]
fn every_leader_group_has_a_name() {
    // An unnamed group is an empty heading in the which-key panel.
    let map = defaults();
    for prefix in [
        "<Leader>b",
        "<Leader>f",
        "<Leader>g",
        "<Leader>l",
        "<Leader>S",
        "<Leader>t",
        "<Leader>u",
        "<Leader>x",
    ] {
        assert!(
            map.group_label(Mode::Normal, &keys(prefix)).is_some(),
            "{prefix} has no name"
        );
    }
}

#[test]
fn motions_are_bound_wherever_a_motion_can_be_typed() {
    // A motion that worked in normal mode and not after an operator would make `dw` impossible.
    let map = defaults();
    for sequence in ["w", "b", "e", "0", "$", "gg", "G", "}", "%"] {
        for mode in [Mode::Normal, Mode::Visual, Mode::OperatorPending] {
            assert!(
                action(&map, mode, sequence).is_some(),
                "{sequence} is not bound in {mode:?}"
            );
        }
    }
}

#[test]
fn text_objects_are_bound_where_they_are_typed_and_nowhere_else() {
    let map = defaults();
    for sequence in ["iw", "ap", "i\"", "a(", "i{"] {
        assert!(
            action(&map, Mode::OperatorPending, sequence).is_some(),
            "{sequence} is not bound after an operator"
        );
        assert!(
            action(&map, Mode::Visual, sequence).is_some(),
            "{sequence} is not bound in visual"
        );
    }
    // `i` in normal mode is insert, not the start of a text object.
    assert_eq!(
        action(&map, Mode::Normal, "i"),
        Some("mode.insert".to_owned())
    );
}

#[test]
fn escape_goes_back_to_normal_from_everywhere_but_a_terminal() {
    // The one binding a modal editor cannot be missing. A terminal is the exception, and vim's:
    // while a program has the keys, `<Esc>` is the program's. A shell whose escape key was taken
    // away is unusable.
    let map = defaults();
    for mode in [
        Mode::Normal,
        Mode::Insert,
        Mode::Replace,
        Mode::Visual,
        Mode::VisualLine,
        Mode::VisualBlock,
        Mode::Select,
        Mode::OperatorPending,
        Mode::Command,
    ] {
        assert_eq!(
            action(&map, mode, "<Esc>"),
            Some("mode.normal".to_owned()),
            "{mode:?}"
        );
    }
    assert_eq!(action(&map, Mode::Terminal, "<Esc>"), None);
}

#[test]
fn no_binding_shadows_a_prefix_it_shares() {
    // A key bound to something *and* the start of a longer sequence in the same mode would make
    // the longer one unreachable, because a whole binding wins over a prefix. The doubled forms
    // `dd`, `yy` and `gcc` belong to the grammar for exactly this reason. An operator that sees
    // its own last key again acts on the current line.
    let map = defaults();
    for mode in [
        Mode::Normal,
        Mode::Visual,
        Mode::OperatorPending,
        Mode::Insert,
    ] {
        let bindings = map.bindings(mode);
        let sequences: Vec<&Vec<zdt_vim::Chord>> = bindings.iter().map(|(keys, _)| keys).collect();
        for keys in &sequences {
            for other in &sequences {
                if other.len() > keys.len() && other.starts_with(keys) {
                    panic!(
                        "in {mode:?}, {} is bound and also starts {}",
                        zdt_vim::notation::format(keys),
                        zdt_vim::notation::format(other),
                    );
                }
            }
        }
    }
}

#[test]
fn every_binding_says_what_it_is_for() {
    // Which-key shows the description; an empty one is a blank row.
    let map = defaults();
    for mode in [Mode::Normal, Mode::Visual, Mode::OperatorPending] {
        for (keys, binding) in map.bindings(mode) {
            assert!(
                !binding.description.trim().is_empty(),
                "{} has no description",
                zdt_vim::notation::format(&keys)
            );
        }
    }
}

#[test]
fn a_users_file_replaces_a_shipped_binding() {
    // The whole reason the defaults are a file: this is what a person's own keymap does to them.
    let mut map = defaults();
    merge(
        &mut map,
        r#"
        [[map]]
        keys = "<Leader>ff"
        action = "picker.git_files"

        [[map]]
        keys = "s"
        mode = ["n", "x", "o"]
        action = false
        "#,
        Leaders::default(),
    )
    .expect("the override reads");

    assert_eq!(
        action(&map, Mode::Normal, "<Leader>ff"),
        Some("picker.git_files".to_owned())
    );
    assert_eq!(
        map.resolve(Mode::Normal, &keys("s")),
        Resolution::None,
        "leap was taken off `s`"
    );
}

#[test]
fn a_different_leader_moves_the_whole_leader_map() {
    let mut map = Keymap::new();
    // A key the defaults do not otherwise use, so this asserts the substitution itself. Two
    // bindings would make it assert which one won.
    let leaders = Leaders {
        leader: zdt_vim::Chord::char('Q'),
        local: zdt_vim::Chord::char(','),
    };
    merge(&mut map, DEFAULTS, leaders).expect("the keymap reads with another leader");

    match map.resolve(Mode::Normal, &[zdt_vim::Chord::char('Q')]) {
        Resolution::Pending(_) => {}
        other => panic!("the leader map did not move: {other:?}"),
    }
}
