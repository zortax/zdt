//! The vim grammar, driven by writing down what somebody typed.
//!
//! A tiny editor built out of a rope and the engine's effects, and a table of cases: start with
//! this text and this caret, type these keys, and this is what the text and the caret must be.
//! Nothing here touches a window, which is the whole reason the engine is a separate crate.
//!
//! Text is written with `|` where the caret is, so a case reads as what a person would see.

use ropey::Rope;
use zdt_vim::config::merge;
use zdt_vim::effect::{Context, Effect, Selection, Step};
use zdt_vim::engine::Engine;
use zdt_vim::keymap::{Keymap, Layered};
use zdt_vim::motion::View;
use zdt_vim::notation::{Leaders, parse};

/// The shipped keymap, which is what the tests are written against.
const DEFAULTS: &str = include_str!("../../../assets/keymap.toml");

/// A rope, an engine, and just enough of an editor to apply what the engine asks for.
struct Editor {
    rope: Rope,
    selections: Vec<Selection>,
    engine: Engine,
    keymap: Keymap,
    undo: Vec<(Rope, Vec<Selection>)>,
    redo: Vec<(Rope, Vec<Selection>)>,
    said: Vec<String>,
    app: Vec<String>,
}

impl Editor {
    /// An editor over `marked`, which is text with a `|` where the caret is.
    fn new(marked: &str) -> Self {
        let at = marked.find('|').unwrap_or(0);
        let text = marked.replacen('|', "", 1);
        let mut keymap = Keymap::new();
        merge(&mut keymap, DEFAULTS, Leaders::default()).expect("the shipped keymap reads");

        Self {
            rope: Rope::from_str(&text),
            selections: vec![Selection::caret(at)],
            engine: Engine::new(),
            keymap,
            undo: Vec::new(),
            redo: Vec::new(),
            said: Vec::new(),
            app: Vec::new(),
        }
    }

    /// Types `sequence`, in vim notation.
    fn keys(&mut self, sequence: &str) -> &mut Self {
        let chords = parse(sequence, Leaders::default()).expect("the sequence reads");
        for chord in chords {
            let step = {
                let context = Context {
                    rope: &self.rope,
                    selections: &self.selections,
                    view: View {
                        top_line: 0,
                        height: 10,
                    },
                };
                self.engine
                    .key(chord, &Layered::plain(&self.keymap), &context)
            };
            match step {
                Step::Consumed(effects) => self.apply(effects),
                Step::Pending => {}
                // Insert-mode typing is the editor's, so this stands in for it.
                Step::PassThrough => self.insert(chord),
            }
        }
        self
    }

    /// What the editor would have done with a key the engine did not want.
    fn insert(&mut self, chord: zdt_vim::Chord) {
        use zdt_vim::chord::{Key, Named};
        let text = match chord.key {
            _ if !chord.mods.without(zdt_vim::Mods::SHIFT).is_empty() => return,
            Key::Char(character) => character.to_string(),
            Key::Named(Named::Enter) => "\n".to_owned(),
            Key::Named(Named::Space) => " ".to_owned(),
            Key::Named(Named::Tab) => "\t".to_owned(),
            Key::Named(Named::Backspace) => {
                let at = self.cursor();
                if at == 0 {
                    return;
                }
                let from = zdt_vim::text::prev_grapheme(&self.rope, at);
                self.replace(vec![(from..at, String::new())]);
                self.selections = vec![Selection::caret(from)];
                return;
            }
            Key::Named(_) => return,
        };
        // Every caret, because the editor this stands in for is multi-caret and a block insert is
        // what that is for.
        let mut carets: Vec<usize> = self.selections.iter().map(|one| one.head).collect();
        carets.sort_unstable();
        self.replace(carets.iter().map(|at| (*at..*at, text.clone())).collect());
        let mut moved = 0;
        self.selections = carets
            .into_iter()
            .map(|at| {
                moved += text.len();
                Selection::caret(at + moved)
            })
            .collect();
    }

    /// Does what the engine asked for.
    fn apply(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::Select(selections) => {
                    if !selections.is_empty() {
                        self.selections = selections;
                    }
                }
                Effect::Replace(replacements) => self.replace(replacements),
                Effect::Undo => {
                    if let Some((rope, selections)) = self.undo.pop() {
                        self.redo.push((self.rope.clone(), self.selections.clone()));
                        self.rope = rope;
                        self.selections = selections;
                    }
                }
                Effect::Redo => {
                    if let Some((rope, selections)) = self.redo.pop() {
                        self.undo.push((self.rope.clone(), self.selections.clone()));
                        self.rope = rope;
                        self.selections = selections;
                    }
                }
                Effect::Say(text) | Effect::Complain(text) => self.said.push(text),
                Effect::App(action) => {
                    // The one action the engine hands back that a test has to carry out itself.
                    if action.name == "vim.replay" {
                        let keys = action.args.str("keys").unwrap_or_default().to_owned();
                        self.engine.begin_replay();
                        self.keys(&keys);
                        self.engine.end_replay();
                    } else {
                        self.app.push(action.name);
                    }
                }
                Effect::Mode(_) | Effect::Scroll(_) => {}
                Effect::SetClipboard { .. } | Effect::ReadClipboard { .. } => {}
            }
        }
    }

    /// Applies replacements, highest first so the offsets stay right.
    fn replace(&mut self, mut replacements: Vec<(std::ops::Range<usize>, String)>) {
        if replacements.is_empty() {
            return;
        }
        self.undo.push((self.rope.clone(), self.selections.clone()));
        self.redo.clear();
        replacements.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
        for (range, text) in replacements {
            let start = self
                .rope
                .byte_to_char(range.start.min(self.rope.len_bytes()));
            let end = self.rope.byte_to_char(range.end.min(self.rope.len_bytes()));
            self.rope.remove(start..end);
            self.rope.insert(start, &text);
        }
    }

    fn cursor(&self) -> usize {
        self.selections.first().map_or(0, |one| one.head)
    }

    fn text(&self) -> String {
        self.rope.to_string()
    }

    /// The text with a `|` where the caret is, which is how a case is written.
    fn rendered(&self) -> String {
        let mut text = self.rope.to_string();
        let at = self.cursor().min(text.len());
        text.insert(at, '|');
        text
    }

    fn mode(&self) -> zdt_vim::Mode {
        self.engine.mode()
    }
}

/// Types `keys` into `before` and asserts the result is `after`.
#[track_caller]
fn case(before: &str, keys: &str, after: &str) {
    let mut editor = Editor::new(before);
    editor.keys(keys);
    assert_eq!(
        editor.rendered(),
        after,
        "\n  from: {before:?}\n  typed: {keys:?}"
    );
}

// ---- Motions -------------------------------------------------------------------------------

#[test]
fn the_letters_move_the_caret() {
    case("|hello", "l", "h|ello");
    case("hell|o", "h", "hel|lo");
    case("|hello", "lll", "hel|lo");
    case("|hello", "3l", "hel|lo");
    case("|hello", "99l", "hell|o");
    case("|hello", "h", "|hello");
}

#[test]
fn the_caret_never_sits_on_the_break() {
    case("|ab\ncd", "lll", "a|b\ncd");
}

#[test]
fn words_move_by_words() {
    case("|hello brave world", "w", "hello |brave world");
    case("|hello brave world", "2w", "hello brave |world");
    case("hello brave |world", "b", "hello |brave world");
    case("|hello", "e", "hell|o");
    case("|foo.bar", "w", "foo|.bar");
    case("|foo.bar", "W", "foo.ba|r");
}

#[test]
fn the_ends_of_a_line_are_reachable() {
    case("    let |x = 1;", "0", "|    let x = 1;");
    case("    let |x = 1;", "^", "    |let x = 1;");
    case("    |let x = 1;", "$", "    let x = 1|;");
}

#[test]
fn gg_and_capital_g_go_to_the_ends_of_the_file() {
    case("one\ntwo\nth|ree", "gg", "|one\ntwo\nthree");
    case("|one\ntwo\nthree", "G", "one\ntwo\n|three");
    case("|one\n  two\nthree", "2G", "one\n  |two\nthree");
}

#[test]
fn a_run_of_j_keeps_its_column() {
    // Through a short line and back, which is the whole reason a goal column exists.
    let mut editor = Editor::new("longer li|ne\nab\nanother long one");
    editor.keys("jj");
    assert_eq!(editor.rendered(), "longer line\nab\nanother l|ong one");
}

#[test]
fn f_and_t_find_a_character_on_the_line() {
    case("|hello world", "fo", "hell|o world");
    case("|hello world", "2fo", "hello w|orld");
    case("|hello world", "to", "hel|lo world");
    case("hello w|orld", "Fo", "hell|o world");
    case("|hello world", "fz", "|hello world");
}

#[test]
fn semicolon_repeats_a_find_and_makes_progress() {
    case("|a.b.c.d", "f.;", "a.b|.c.d");
    // `t.` from the start is already just before the first dot, so `;` walks to just before the
    // second one.
    case("|a.b.c.d", "t.;", "a.|b.c.d");
    // Forward to the next dot, then `,` back to the one it came from.
    case("a.b|.c.d", "f.,", "a.b|.c.d");
}

#[test]
fn percent_jumps_between_brackets() {
    // On a closing bracket it goes to the opening one, and the other way round.
    case(
        "fn main(|) { let x = 1; }",
        "%",
        "fn main|() { let x = 1; }",
    );
    case(
        "fn main() |{ let x = 1; }",
        "%",
        "fn main() { let x = 1; |}",
    );
}

// ---- Operators and motions ------------------------------------------------------------------

#[test]
fn d_takes_what_the_motion_covers() {
    case("|hello world", "dw", "|world");
    case("|hello world", "de", "| world");
    case("hello |world", "d$", "hello |");
    case("hello |world", "db", "|world");
}

#[test]
fn a_count_multiplies_through_an_operator() {
    case("|one two three four", "d2w", "|three four");
    case("|one two three four", "2dw", "|three four");
    // Both counts multiply, which is vim's rule.
    case("|a b c d e f g", "2d3w", "|g");
}

#[test]
fn a_doubled_operator_takes_the_line() {
    case("one\n|two\nthree", "dd", "one\n|three");
    case("|one\ntwo\nthree", "2dd", "|three");
    case("one\n|two\nthree", "yy", "one\n|two\nthree");
}

#[test]
fn dd_on_the_last_line_leaves_no_empty_one_behind() {
    case("one\ntwo\n|three", "dd", "one\n|two");
}

#[test]
fn c_changes_and_leaves_the_editor_inserting() {
    let mut editor = Editor::new("|hello world");
    editor.keys("cw");
    assert_eq!(editor.mode(), zdt_vim::Mode::Insert);
    assert_eq!(editor.rendered(), "|world");
    editor.keys("goodbye<Esc>");
    assert_eq!(editor.text(), "goodbyeworld");
}

#[test]
fn cc_empties_the_line_and_keeps_its_indent() {
    let mut editor = Editor::new("fn main() {\n    let |x = 1;\n}");
    editor.keys("cc");
    assert_eq!(editor.text(), "fn main() {\n    \n}");
    assert_eq!(editor.mode(), zdt_vim::Mode::Insert);
}

#[test]
fn y_then_p_puts_it_back() {
    case("|hello world", "yyp", "hello world\n|hello world");
    // The caret ends on the last character pasted, which is where vim leaves it.
    case("|hello world", "ywP", "hello| hello world");
}

#[test]
fn indenting_moves_the_lines_a_motion_covers() {
    case("|one\ntwo\nthree", ">j", "    |one\n    two\nthree");
    case("    |one\ntwo", "<lt><lt>", "|one\ntwo");
}

#[test]
fn case_operators_change_the_letters_they_cover() {
    case("|hello world", "gUw", "|HELLO world");
    case("|HELLO world", "guw", "|hello world");
    case("|Hello", "g~w", "|hELLO");
}

// ---- Text objects ----------------------------------------------------------------------------

#[test]
fn iw_takes_the_word_the_caret_is_in() {
    case("hello bra|ve world", "diw", "hello | world");
    case("hello bra|ve world", "daw", "hello |world");
}

#[test]
fn quotes_and_brackets_are_text_objects() {
    case("let name = \"hel|lo\";", "ci\"", "let name = \"|\";");
    case("let name = \"hel|lo\";", "da\"", "let name = |;");
    case("fn main() { let |x = 1; }", "di{", "fn main() {|}");
    case("fn main() { let |x = 1; }", "da{", "fn main() |");
}

#[test]
fn a_paragraph_is_a_text_object() {
    case("one\ntw|o\n\nthree", "dip", "|\nthree");
}

// ---- Edits ------------------------------------------------------------------------------------

#[test]
fn x_deletes_a_character() {
    case("|hello", "x", "|ello");
    case("|hello", "3x", "|lo");
    case("hell|o", "x", "hell|");
    case("h|ello", "X", "|ello");
}

#[test]
fn capital_d_and_c_go_to_the_end_of_the_line() {
    case("hello |world\nnext", "D", "hello |\nnext");
    let mut editor = Editor::new("hello |world");
    editor.keys("Cthere<Esc>");
    assert_eq!(editor.text(), "hello there");
}

#[test]
fn r_replaces_one_character_and_stays_in_normal_mode() {
    let mut editor = Editor::new("|hello");
    editor.keys("rx");
    assert_eq!(editor.rendered(), "|xello");
    assert_eq!(editor.mode(), zdt_vim::Mode::Normal);
    case("|hello", "3rx", "|xxxlo");
}

#[test]
fn tilde_turns_a_letters_case_over() {
    case("|hello", "~", "H|ello");
    case("|hello", "3~", "HEL|lo");
}

#[test]
fn j_joins_the_next_line_on_with_one_space() {
    case("|one\ntwo", "J", "one| two");
    case("|one\n   two", "J", "one| two");
    case("|one \ntwo", "J", "one |two");
    case("|one\ntwo", "gJ", "one|two");
    // A count joins that many lines, and the caret ends on the last join.
    case("|one\ntwo\nthree", "3J", "one two| three");
    case("|one\ntwo", "5J", "one| two");
}

#[test]
fn the_insert_commands_start_where_they_say() {
    let mut editor = Editor::new("hel|lo");
    editor.keys("iX<Esc>");
    assert_eq!(editor.text(), "helXlo");

    let mut editor = Editor::new("hel|lo");
    editor.keys("aX<Esc>");
    assert_eq!(editor.text(), "hellXo");

    let mut editor = Editor::new("  hel|lo");
    editor.keys("IX<Esc>");
    assert_eq!(editor.text(), "  Xhello");

    let mut editor = Editor::new("hel|lo");
    editor.keys("AX<Esc>");
    assert_eq!(editor.text(), "helloX");
}

#[test]
fn o_opens_a_line_and_keeps_the_indent() {
    let mut editor = Editor::new("fn main() {\n    let |x = 1;\n}");
    editor.keys("olet y = 2;<Esc>");
    assert_eq!(
        editor.text(),
        "fn main() {\n    let x = 1;\n    let y = 2;\n}"
    );

    let mut editor = Editor::new("fn main() {\n    let |x = 1;\n}");
    editor.keys("Olet w = 0;<Esc>");
    assert_eq!(
        editor.text(),
        "fn main() {\n    let w = 0;\n    let x = 1;\n}"
    );
}

#[test]
fn escape_leaves_the_caret_where_vim_leaves_it() {
    // On the last character typed, and not after it.
    let mut editor = Editor::new("|hello");
    editor.keys("iXY<Esc>");
    assert_eq!(editor.rendered(), "X|Yhello");
}

#[test]
fn undo_and_redo_walk_the_history() {
    let mut editor = Editor::new("|hello world");
    editor.keys("dw");
    assert_eq!(editor.text(), "world");
    editor.keys("u");
    assert_eq!(editor.text(), "hello world");
    editor.keys("<C-r>");
    assert_eq!(editor.text(), "world");
}

// ---- Registers ---------------------------------------------------------------------------------

#[test]
fn a_named_register_keeps_what_it_was_given() {
    let mut editor = Editor::new("|one\ntwo\nthree");
    editor.keys("\"ayy");
    editor.keys("jdd");
    editor.keys("\"ap");
    // The buffer had no trailing break and does not gain one.
    assert_eq!(editor.text(), "one\nthree\none");
}

#[test]
fn the_yank_register_survives_a_delete() {
    // `"0p` after a delete is the single most useful thing about vim's registers.
    let mut editor = Editor::new("|keep\nthrow");
    editor.keys("yy");
    editor.keys("jdd");
    editor.keys("\"0p");
    assert_eq!(editor.text(), "keep\nkeep");
}

#[test]
fn the_black_hole_throws_away_without_touching_the_others() {
    let mut editor = Editor::new("|keep\nthrow\nlast");
    editor.keys("yy");
    editor.keys("j\"_dd");
    editor.keys("p");
    assert_eq!(editor.text(), "keep\nlast\nkeep");
}

// ---- Visual modes ---------------------------------------------------------------------------------

#[test]
fn visual_selects_and_an_operator_takes_the_selection() {
    case("|hello world", "vlld", "|lo world");
    case("|hello world", "vwd", "|orld");
}

#[test]
fn visual_line_takes_whole_lines() {
    case("one\n|two\nthree", "Vd", "one\n|three");
    case("|one\ntwo\nthree", "Vjd", "|three");
}

#[test]
fn a_visual_mode_typed_again_leaves_it() {
    let mut editor = Editor::new("|hello");
    editor.keys("v");
    assert_eq!(editor.mode(), zdt_vim::Mode::Visual);
    editor.keys("v");
    assert_eq!(editor.mode(), zdt_vim::Mode::Normal);
}

#[test]
fn escape_leaves_visual_mode() {
    let mut editor = Editor::new("|hello");
    editor.keys("vll<Esc>");
    assert_eq!(editor.mode(), zdt_vim::Mode::Normal);
    assert_eq!(editor.text(), "hello");
}

#[test]
fn a_text_object_in_visual_mode_selects_it() {
    case("hello bra|ve world", "viwd", "hello | world");
}

#[test]
fn visual_block_deletes_a_rectangle() {
    // The whole reason block mode exists.
    let mut editor = Editor::new("|abcd\nabcd\nabcd");
    editor.keys("<C-v>jjld");
    assert_eq!(editor.text(), "cd\ncd\ncd");
}

#[test]
fn visual_block_insert_types_on_every_line() {
    let mut editor = Editor::new("|one\ntwo\nthree");
    editor.keys("<C-v>jjI- <Esc>");
    assert_eq!(editor.text(), "- one\n- two\n- three");
}

// ---- Marks and jumps ------------------------------------------------------------------------------

#[test]
fn a_mark_is_somewhere_to_come_back_to() {
    let mut editor = Editor::new("|one\ntwo\nthree");
    editor.keys("majj");
    assert_eq!(editor.rendered(), "one\ntwo\n|three");
    editor.keys("`a");
    assert_eq!(editor.rendered(), "|one\ntwo\nthree");
}

#[test]
fn a_jump_can_be_walked_back() {
    let mut editor = Editor::new("|one\ntwo\nthree");
    editor.keys("G");
    assert_eq!(editor.rendered(), "one\ntwo\n|three");
    editor.keys("<C-o>");
    assert_eq!(editor.rendered(), "|one\ntwo\nthree");
    editor.keys("<C-i>");
    assert_eq!(editor.rendered(), "one\ntwo\n|three");
}

// ---- Repeat and macros ------------------------------------------------------------------------------

#[test]
fn the_dot_puts_the_last_change_back() {
    let mut editor = Editor::new("|one two three four");
    editor.keys("dw");
    assert_eq!(editor.text(), "two three four");
    editor.keys(".");
    assert_eq!(editor.text(), "three four");
    editor.keys("..");
    assert_eq!(editor.text(), "");
}

#[test]
fn the_dot_repeats_the_change_rather_than_the_motion_after_it() {
    // After `dw` and an `l`, `.` deletes a word again, at the caret, where vim repeats it. A `.`
    // that repeated the `l` would leave the text alone.
    let mut editor = Editor::new("|one two three");
    editor.keys("dw");
    assert_eq!(editor.text(), "two three");
    editor.keys("l");
    editor.keys(".");
    assert_eq!(editor.text(), "tthree");
}

#[test]
fn a_macro_replays_what_was_recorded() {
    let mut editor = Editor::new("|a\nb\nc\nd");
    editor.keys("qqI- <Esc>jq");
    assert_eq!(editor.text(), "- a\nb\nc\nd");
    editor.keys("@q");
    assert_eq!(editor.text(), "- a\n- b\nc\nd");
    editor.keys("@@");
    assert_eq!(editor.text(), "- a\n- b\n- c\nd");
}

// ---- What is not the engine's ----------------------------------------------------------------------

#[test]
fn an_action_the_engine_does_not_know_reaches_the_application() {
    let mut editor = Editor::new("|hello");
    editor.keys("<Space>ff");
    assert_eq!(editor.app, vec!["picker.files"]);
}

#[test]
fn a_sequence_that_is_bound_to_nothing_is_harmless() {
    let mut editor = Editor::new("|hello");
    editor.keys("gq");
    assert_eq!(editor.text(), "hello");
    editor.keys("dw");
    assert_eq!(editor.text(), "", "and the editor still works afterwards");
}

#[test]
fn escape_backs_out_of_a_pending_operator() {
    let mut editor = Editor::new("|hello world");
    editor.keys("d<Esc>");
    assert_eq!(editor.mode(), zdt_vim::Mode::Normal);
    editor.keys("x");
    assert_eq!(editor.text(), "ello world", "the `d` was forgotten");
}

#[test]
fn a_count_survives_being_typed_before_a_register() {
    let mut editor = Editor::new("|one\ntwo\nthree");
    editor.keys("\"a2yy");
    editor.keys("G\"ap");
    assert_eq!(editor.text(), "one\ntwo\nthree\none\ntwo");
}
