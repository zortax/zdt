//! Keys taken as themselves: insert mode, replace mode, and `<C-v>`.

use super::*;

impl Engine {
    /// A key the engine asked for by itself, which no keymap gets a say in.
    pub(super) fn literal(&mut self, waiting: Awaiting, chord: Chord, cx: &Context<'_>) -> Step {
        // Escape backs out of anything that was waiting for a key.
        if chord == Chord::named(Named::Escape) {
            self.operator = None;
            self.mode = if self.mode == Mode::OperatorPending {
                Mode::Normal
            } else {
                self.mode
            };
            return Step::one(Effect::Mode(self.mode));
        }

        match waiting {
            Awaiting::FindChar {
                backward,
                till,
                count,
            } => {
                let Some(character) = chord.inserted() else {
                    return Step::nothing();
                };
                let find = FindChar {
                    character,
                    backward,
                    till,
                };
                self.last_find = Some(find);
                match motion::find_char(cx.rope, self.caret(cx), count, find, false) {
                    Some(target) => self.go(target, cx),
                    None => {
                        self.operator = None;
                        Step::nothing()
                    }
                }
            }
            Awaiting::ReplaceChar { count } => {
                let Some(character) = chord.inserted() else {
                    return Step::nothing();
                };
                let rope = cx.rope;
                let at = self.caret(cx);
                let mut end = at;
                for _ in 0..count {
                    end = text::next_grapheme(rope, end);
                }
                let end = end.min(text::line_end(rope, text::line_of(rope, at)));
                if end <= at {
                    return Step::nothing();
                }
                let replacement: String =
                    std::iter::repeat_n(character, count.max(1) as usize).collect();
                Step::Consumed(vec![
                    Effect::Replace(vec![(at..end, replacement)]),
                    Effect::Select(vec![Selection::caret(at)]),
                ])
            }
            Awaiting::SetMark => {
                let Some(character) = chord.inserted() else {
                    return Step::nothing();
                };
                self.marks.insert(character, self.caret(cx));
                Step::nothing()
            }
            Awaiting::JumpMark { line } => {
                let Some(character) = chord.inserted() else {
                    return Step::nothing();
                };
                let Some(byte) = self.marks.get(&character).copied() else {
                    return Step::one(Effect::Complain(format!("no mark {character}")));
                };
                let byte = byte.min(cx.rope.len_bytes());
                let byte = if line {
                    text::first_non_blank(cx.rope, text::line_of(cx.rope, byte))
                } else {
                    byte
                };
                self.jumps.push(self.caret(cx));
                Step::Consumed(vec![
                    Effect::Select(vec![Selection::caret(text::clamp_normal(cx.rope, byte))]),
                    Effect::Scroll(Scroll::EnsureVisible),
                ])
            }
            Awaiting::Register => {
                let Some(character) = chord.inserted() else {
                    return Step::nothing();
                };
                match Name::of(character) {
                    Some(name) => {
                        self.register = Some(name);
                        Step::Pending
                    }
                    None => Step::one(Effect::Complain(format!("no register {character}"))),
                }
            }
            Awaiting::RecordMacro => {
                let Some(character) = chord.inserted() else {
                    return Step::nothing();
                };
                if !character.is_ascii_alphanumeric() {
                    return Step::one(Effect::Complain(format!("cannot record into {character}")));
                }
                self.recording_macro = Some((character, Vec::new()));
                Step::one(Effect::Say(format!("recording @{character}")))
            }
            Awaiting::PlayMacro => {
                let Some(character) = chord.inserted() else {
                    return Step::nothing();
                };
                let name = if character == '@' {
                    match self.last_macro {
                        Some(name) => name,
                        None => return Step::nothing(),
                    }
                } else {
                    character
                };
                self.last_macro = Some(name);
                if self.macro_depth >= MACRO_DEPTH {
                    return Step::one(Effect::Complain("macros are nested too deeply".to_owned()));
                }
                let Some(name) = Name::of(name) else {
                    return Step::nothing();
                };
                let keys = self.registers.get(name).text;
                if keys.is_empty() {
                    return Step::nothing();
                }
                // The application replays them. Replaying needs the buffer as it is after each
                // key, and this sees it only as it was when the macro started.
                Step::one(Effect::App(Action::with(
                    "vim.replay",
                    Args::new(
                        [("keys".to_owned(), toml::Value::String(keys))]
                            .into_iter()
                            .collect(),
                    ),
                )))
            }
        }
    }
}
