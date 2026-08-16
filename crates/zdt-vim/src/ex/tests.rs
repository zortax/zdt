use super::*;

fn rope() -> Rope {
    Rope::from_str("one\ntwo\nthree\nfour\nfive\n")
}

#[test]
fn a_bare_number_is_a_jump() {
    assert_eq!(parse(":42"), Some(Command::Goto(42)));
    assert_eq!(parse("7"), Some(Command::Goto(7)));
}

#[test]
fn the_common_commands_read() {
    assert_eq!(
        parse(":w"),
        Some(Command::Write {
            path: None,
            force: false,
            then_quit: false,
            all: false
        })
    );
    assert_eq!(
        parse(":wq"),
        Some(Command::Write {
            path: None,
            force: false,
            then_quit: true,
            all: false
        })
    );
    assert_eq!(
        parse(":q"),
        Some(Command::Quit {
            force: false,
            all: false
        })
    );
    assert_eq!(
        parse(":q!"),
        Some(Command::Quit {
            force: true,
            all: false
        })
    );
    assert_eq!(
        parse(":qa"),
        Some(Command::Quit {
            force: false,
            all: true
        })
    );
    assert_eq!(parse(":noh"), Some(Command::NoHighlight));
}

#[test]
fn a_path_after_a_command_is_its_argument() {
    assert_eq!(
        parse(":w src/main.rs"),
        Some(Command::Write {
            path: Some("src/main.rs".to_owned()),
            force: false,
            then_quit: false,
            all: false
        })
    );
    assert_eq!(
        parse(":e ../other.txt"),
        Some(Command::Edit {
            path: Some("../other.txt".to_owned()),
            force: false
        })
    );
}

#[test]
fn splitting_reads_both_ways_round() {
    assert_eq!(
        parse(":sp"),
        Some(Command::Split {
            vertical: false,
            path: None
        })
    );
    assert_eq!(
        parse(":vs a.rs"),
        Some(Command::Split {
            vertical: true,
            path: Some("a.rs".to_owned())
        })
    );
}

#[test]
fn the_buffer_commands_read() {
    assert_eq!(parse(":bn"), Some(Command::Buffer(BufferTarget::Next)));
    assert_eq!(parse(":bp"), Some(Command::Buffer(BufferTarget::Previous)));
    assert_eq!(
        parse(":b main"),
        Some(Command::Buffer(BufferTarget::Named("main".to_owned())))
    );
    assert_eq!(parse(":bd"), Some(Command::BufferDelete { force: false }));
    assert_eq!(parse(":bd!"), Some(Command::BufferDelete { force: true }));
}

#[test]
fn a_substitution_reads_its_three_parts() {
    let Some(Command::Substitute {
        pattern,
        replacement,
        all,
        ..
    }) = parse(":%s/old/new/g")
    else {
        panic!("it is a substitution");
    };
    assert_eq!(pattern, "old");
    assert_eq!(replacement, "new");
    assert!(all);
}

#[test]
fn any_character_can_separate_a_substitution() {
    // Which is the point of allowing it: a path is full of slashes.
    let Some(Command::Substitute {
        pattern,
        replacement,
        ..
    }) = parse(":s#/usr/bin#/usr/local/bin#")
    else {
        panic!("it is a substitution");
    };
    assert_eq!(pattern, "/usr/bin");
    assert_eq!(replacement, "/usr/local/bin");
}

#[test]
fn an_escaped_separator_is_part_of_the_pattern() {
    let Some(Command::Substitute {
        pattern,
        replacement,
        ..
    }) = parse(r":s/a\/b/c/")
    else {
        panic!("it is a substitution");
    };
    assert_eq!(pattern, "a/b");
    assert_eq!(replacement, "c");
}

#[test]
fn a_substitution_with_no_flags_replaces_the_first_on_each_line() {
    let Some(Command::Substitute {
        all, ignore_case, ..
    }) = parse(":s/a/b/")
    else {
        panic!("it is a substitution");
    };
    assert!(!all);
    assert!(!ignore_case);
}

#[test]
fn the_whole_file_is_a_range_of_one_character() {
    let Some(Command::Substitute { range, .. }) = parse(":%s/a/b/") else {
        panic!("it is a substitution");
    };
    assert_eq!(range.lines(&rope(), 0, |_| None), 0..=4);
}

#[test]
fn a_range_of_numbers_reads_and_resolves() {
    let Some(Command::Substitute { range, .. }) = parse(":2,4s/a/b/") else {
        panic!("it is a substitution");
    };
    assert_eq!(range.lines(&rope(), 0, |_| None), 1..=3);
}

#[test]
fn a_range_with_one_end_is_that_line() {
    let Some(Command::Substitute { range, .. }) = parse(":3s/a/b/") else {
        panic!("it is a substitution");
    };
    assert_eq!(range.lines(&rope(), 0, |_| None), 2..=2);
}

#[test]
fn the_caret_and_the_end_are_addresses() {
    let Some(Command::Substitute { range, .. }) = parse(":.,$s/a/b/") else {
        panic!("it is a substitution");
    };
    assert_eq!(range.lines(&rope(), 1, |_| None), 1..=4);
}

#[test]
fn a_backwards_range_is_read_the_right_way_round() {
    let Some(Command::Substitute { range, .. }) = parse(":4,2s/a/b/") else {
        panic!("it is a substitution");
    };
    assert_eq!(range.lines(&rope(), 0, |_| None), 1..=3);
}

#[test]
fn a_range_past_the_end_is_clamped() {
    let Some(Command::Substitute { range, .. }) = parse(":1,900s/a/b/") else {
        panic!("it is a substitution");
    };
    assert_eq!(range.lines(&rope(), 0, |_| None), 0..=4);
}

#[test]
fn a_mark_is_an_address() {
    let Some(Command::Substitute { range, .. }) = parse(":'a,'bs/x/y/") else {
        panic!("it is a substitution");
    };
    assert_eq!(
        range.lines(&rope(), 0, |name| match name {
            'a' => Some(1),
            'b' => Some(3),
            _ => None,
        }),
        1..=3
    );
}

#[test]
fn a_pattern_is_an_address() {
    let Some(Command::Substitute { range, .. }) = parse(":/four/s/a/b/") else {
        panic!("it is a substitution");
    };
    assert_eq!(range.lines(&rope(), 0, |_| None), 3..=3);
}

#[test]
fn a_setting_reads_its_three_shapes() {
    assert_eq!(
        parse(":set number"),
        Some(Command::Set {
            name: "number".to_owned(),
            value: None,
            off: false,
            toggle: false
        })
    );
    assert_eq!(
        parse(":set nonumber"),
        Some(Command::Set {
            name: "number".to_owned(),
            value: None,
            off: true,
            toggle: false
        })
    );
    assert_eq!(
        parse(":set number!"),
        Some(Command::Set {
            name: "number".to_owned(),
            value: None,
            off: false,
            toggle: true
        })
    );
    assert_eq!(
        parse(":set tabstop=4"),
        Some(Command::Set {
            name: "tabstop".to_owned(),
            value: Some("4".to_owned()),
            off: false,
            toggle: false
        })
    );
}

#[test]
fn a_setting_whose_name_begins_with_no_is_not_a_negation() {
    assert_eq!(
        parse(":set noise=2"),
        Some(Command::Set {
            name: "noise".to_owned(),
            value: Some("2".to_owned()),
            off: false,
            toggle: false
        }),
        "a value means the name was meant"
    );
}

#[test]
fn a_shell_line_is_read_whole() {
    assert_eq!(
        parse(":!cargo test --workspace"),
        Some(Command::Shell("cargo test --workspace".to_owned()))
    );
}

#[test]
fn something_unknown_says_so_rather_than_guessing() {
    assert_eq!(
        parse(":frobnicate"),
        Some(Command::Unknown("frobnicate".to_owned()))
    );
}

#[test]
fn an_empty_line_is_nothing() {
    assert_eq!(parse(":"), None);
    assert_eq!(parse("   "), None);
    assert_eq!(parse(""), None);
}

#[test]
fn a_range_with_no_command_and_no_number_is_nothing() {
    // `:.` on its own is a jump to where the caret already is, which is not a command.
    assert_eq!(parse(":."), None);
}
