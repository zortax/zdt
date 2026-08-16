use ropey::Rope;

use super::{
    FindChar, Kind, View, byte_at_column, column_of, document_end, find_char, first_non_blank,
    goto_line, half_page_down, last_non_blank, left, line_end, line_start, matching_bracket,
    paragraph_backward, paragraph_forward, right, screen_bottom, screen_middle, screen_top,
    word_backward, word_end, word_end_backward, word_forward,
};

fn rope(text: &str) -> Rope {
    Rope::from_str(text)
}

#[test]
fn h_and_l_stay_on_their_line() {
    // Which is what makes them safe to hold down.
    let rope = rope("ab\ncd");
    assert_eq!(left(&rope, 0, 1).byte, 0);
    assert_eq!(right(&rope, 2, 1).byte, 2);
    assert_eq!(right(&rope, 0, 1).byte, 1);
    assert_eq!(left(&rope, 2, 1).byte, 1);
}

#[test]
fn a_count_moves_that_many_times() {
    let rope = rope("abcdef");
    assert_eq!(right(&rope, 0, 3).byte, 3);
    assert_eq!(left(&rope, 5, 2).byte, 3);
}

#[test]
fn a_word_forward_lands_on_the_next_words_start() {
    let rope = rope("foo bar baz");
    assert_eq!(word_forward(&rope, 0, 1, false).byte, 4);
    assert_eq!(word_forward(&rope, 0, 2, false).byte, 8);
}

#[test]
fn a_small_word_stops_at_punctuation_and_a_big_one_does_not() {
    let rope = rope("foo.bar baz");
    assert_eq!(word_forward(&rope, 0, 1, false).byte, 3, "the dot");
    assert_eq!(word_forward(&rope, 0, 1, true).byte, 8, "past it all");
}

#[test]
fn a_word_backward_lands_on_the_previous_words_start() {
    let rope = rope("foo bar baz");
    assert_eq!(word_backward(&rope, 8, 1, false).byte, 4);
    assert_eq!(word_backward(&rope, 8, 2, false).byte, 0);
    assert_eq!(word_backward(&rope, 0, 1, false).byte, 0);
}

#[test]
fn a_word_end_is_inclusive_which_is_what_makes_de_different_from_dw() {
    let rope = rope("foo bar");
    let target = word_end(&rope, 0, 1, false);
    assert_eq!(target.byte, 2, "the second `o`");
    assert_eq!(target.kind, Kind::Inclusive);
    assert_eq!(word_forward(&rope, 0, 1, false).kind, Kind::Exclusive);
}

#[test]
fn a_word_end_from_the_end_of_a_word_goes_to_the_next_one() {
    let rope = rope("foo bar");
    assert_eq!(word_end(&rope, 2, 1, false).byte, 6);
}

#[test]
fn ge_goes_back_to_the_previous_words_end() {
    let rope = rope("foo bar");
    assert_eq!(word_end_backward(&rope, 4, 1, false).byte, 2);
}

#[test]
fn a_word_motion_crosses_lines() {
    let rope = rope("foo\nbar");
    assert_eq!(word_forward(&rope, 0, 1, false).byte, 4);
    assert_eq!(word_backward(&rope, 4, 1, false).byte, 0);
}

#[test]
fn an_empty_line_is_a_word_of_its_own() {
    // That is what makes `w` stop on a blank line.
    let rope = rope("foo\n\nbar");
    assert_eq!(word_forward(&rope, 0, 1, false).byte, 4);
    assert_eq!(word_forward(&rope, 4, 1, false).byte, 5);
}

#[test]
fn the_ends_of_a_line_are_where_they_should_be() {
    let rope = rope("    let x = 1;\nnext");
    assert_eq!(line_start(&rope, 6).byte, 0);
    assert_eq!(first_non_blank(&rope, 6).byte, 4);
    let end = line_end(&rope, 0, 1);
    assert_eq!(end.byte, 14);
    assert_eq!(end.kind, Kind::Inclusive, "`d$` takes the last character");
    assert_eq!(last_non_blank(&rope, 0, 1).byte, 13);
}

#[test]
fn a_count_on_dollar_reaches_a_later_lines_end() {
    let rope = rope("one\ntwo\nthree");
    assert_eq!(line_end(&rope, 0, 2).byte, 7);
}

#[test]
fn going_to_a_line_lands_on_its_first_non_blank() {
    let rope = rope("one\n    two\nthree");
    assert_eq!(goto_line(&rope, Some(2)).byte, 8);
    assert!(goto_line(&rope, Some(2)).jump, "the jump list remembers it");
    assert_eq!(goto_line(&rope, None).byte, 0);
}

#[test]
fn capital_g_goes_to_the_last_line_with_something_on_it() {
    // A text ending in a break has an empty line after it that `G` must not land on.
    let rope = rope("one\ntwo\n");
    assert_eq!(document_end(&rope, None).byte, 4);
    assert_eq!(document_end(&rope, Some(1)).byte, 0);
}

#[test]
fn paragraphs_are_the_empty_lines_between_them() {
    let rope = rope("one\ntwo\n\nthree\nfour\n\nfive");
    assert_eq!(paragraph_forward(&rope, 0, 1).byte, 8);
    assert_eq!(paragraph_forward(&rope, 0, 2).byte, 20);
    assert_eq!(paragraph_backward(&rope, 22, 1).byte, 20);
}

#[test]
fn a_bracket_matches_its_partner_either_way() {
    let rope = rope("fn main() { let x = (1 + 2); }");
    let forward = matching_bracket(&rope, 10).expect("the brace matches");
    assert_eq!(forward.byte, 29);
    let backward = matching_bracket(&rope, 29).expect("the brace matches");
    assert_eq!(backward.byte, 10);
    assert_eq!(forward.kind, Kind::Inclusive);
}

#[test]
fn percent_finds_the_first_bracket_after_the_caret() {
    let rope = rope("let x = (1);");
    assert_eq!(
        matching_bracket(&rope, 0).expect("there is one").byte,
        10,
        "it looked forward for the `(` and matched it"
    );
}

#[test]
fn percent_with_no_bracket_on_the_line_goes_nowhere() {
    let rope = rope("let x = 1;\n(nope)");
    assert_eq!(matching_bracket(&rope, 0), None);
}

#[test]
fn a_nested_bracket_matches_the_right_one() {
    let rope = rope("((a))");
    assert_eq!(matching_bracket(&rope, 0).expect("matched").byte, 4);
    assert_eq!(matching_bracket(&rope, 1).expect("matched").byte, 3);
}

#[test]
fn f_lands_on_the_character_and_t_stops_before_it() {
    let rope = rope("hello world");
    let find = |till, backward| FindChar {
        character: 'o',
        backward,
        till,
    };
    assert_eq!(
        find_char(&rope, 0, 1, find(false, false), false)
            .unwrap()
            .byte,
        4
    );
    assert_eq!(
        find_char(&rope, 0, 1, find(true, false), false)
            .unwrap()
            .byte,
        3
    );
    assert_eq!(
        find_char(&rope, 0, 2, find(false, false), false)
            .unwrap()
            .byte,
        7
    );
}

#[test]
fn f_is_inclusive_and_capital_f_is_exclusive() {
    // Vim's rule: `dfx` takes the `x`, `dFx` leaves it.
    let rope = rope("hello world");
    let forward = find_char(
        &rope,
        0,
        1,
        FindChar {
            character: 'o',
            backward: false,
            till: false,
        },
        false,
    )
    .unwrap();
    let backward = find_char(
        &rope,
        7,
        1,
        FindChar {
            character: 'o',
            backward: true,
            till: false,
        },
        false,
    )
    .unwrap();
    assert_eq!(forward.kind, Kind::Inclusive);
    assert_eq!(backward.kind, Kind::Exclusive);
    assert_eq!(backward.byte, 4);
}

#[test]
fn a_find_stops_at_the_end_of_the_line() {
    let rope = rope("abc\nxyz");
    assert_eq!(
        find_char(
            &rope,
            0,
            1,
            FindChar {
                character: 'z',
                backward: false,
                till: false
            },
            false,
        ),
        None
    );
}

#[test]
fn a_count_on_till_counts_the_characters_rather_than_the_steps() {
    let rope = rope("a.b.c");
    let till = FindChar {
        character: '.',
        backward: false,
        till: true,
    };
    // Already just before the first dot, so one of them is no movement.
    assert_eq!(find_char(&rope, 0, 1, till, false).unwrap().byte, 0);
    assert_eq!(find_char(&rope, 0, 2, till, false).unwrap().byte, 2);
}

#[test]
fn a_repeated_till_makes_progress() {
    // `;` after a `t` must walk down the line. Finding the same character again would leave the
    // caret standing still.
    let rope = rope("a.b.c");
    let till = FindChar {
        character: '.',
        backward: false,
        till: true,
    };
    assert_eq!(find_char(&rope, 0, 1, till, true).unwrap().byte, 2);
    assert_eq!(find_char(&rope, 2, 1, till, true), None, "no third dot");
}

#[test]
fn the_screen_motions_read_the_view() {
    let rope = rope("1\n2\n3\n4\n5\n6\n7\n8\n9\n10");
    let view = View {
        top_line: 2,
        height: 5,
    };
    assert_eq!(screen_top(&rope, view, 1).byte, 4, "line 3");
    assert_eq!(screen_bottom(&rope, view, 1).byte, 12, "line 7");
    assert_eq!(screen_middle(&rope, view).byte, 8, "line 5");
    assert_eq!(screen_top(&rope, view, 2).byte, 6, "line 4");
}

#[test]
fn a_column_survives_a_short_line_in_between() {
    // The whole reason a goal column exists: `j` through a short line and back must land
    // where it started.
    let rope = rope("longer line\nab\nanother long one");
    let goal = column_of(&rope, 8);
    let short = super::down(&rope, 8, 1, Some(goal));
    assert_eq!(short.byte, 14, "the short line's end");
    let back = super::down(&rope, short.byte, 1, Some(goal));
    assert_eq!(column_of(&rope, back.byte), 8);
}

#[test]
fn vertical_motions_are_linewise() {
    let rope = rope("one\ntwo\nthree");
    assert_eq!(super::down(&rope, 0, 1, None).kind, Kind::Linewise);
    assert_eq!(super::up(&rope, 4, 1, None).kind, Kind::Linewise);
}

#[test]
fn vertical_motions_stop_at_the_ends_of_the_text() {
    let rope = rope("one\ntwo");
    assert_eq!(super::up(&rope, 0, 5, None).byte, 0);
    assert_eq!(super::down(&rope, 0, 99, None).byte, 4);
}

#[test]
fn a_column_is_counted_in_graphemes() {
    let rope = rope("a\u{1F1E9}\u{1F1EA}b");
    assert_eq!(column_of(&rope, 9), 2);
    assert_eq!(byte_at_column(&rope, 0, 2), 9);
    assert_eq!(byte_at_column(&rope, 0, 99), 10, "the line's end");
}

#[test]
fn half_a_page_is_half_the_view() {
    let rope = rope("1\n2\n3\n4\n5\n6\n7\n8\n9\n10");
    let view = View {
        top_line: 0,
        height: 6,
    };
    assert_eq!(
        half_page_down(&rope, 0, view, 1).byte,
        6,
        "three lines down"
    );
}
