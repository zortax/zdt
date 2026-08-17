//! The edits that are commands of their own: `x`, `J`, `p`, `.`.

use super::*;

impl Engine {
    /// The commands that change text without being an operator and a motion.
    pub(super) fn edit(&mut self, action: &Action, count: u32, cx: &Context<'_>) -> Step {
        let rope = cx.rope;
        let at = self.caret(cx);

        match action.leaf() {
            "undo" => Step::Consumed(vec![Effect::Undo, Effect::Scroll(Scroll::EnsureVisible)]),
            "redo" => Step::Consumed(vec![Effect::Redo, Effect::Scroll(Scroll::EnsureVisible)]),
            "delete_char" => {
                let register = self.take_register();
                let range = if action.args.flag("backward") {
                    let mut start = at;
                    for _ in 0..count {
                        start = text::prev_grapheme(rope, start);
                    }
                    start.max(text::line_start(rope, text::line_of(rope, at)))..at
                } else {
                    let mut end = at;
                    for _ in 0..count {
                        end = text::next_grapheme(rope, end);
                    }
                    at..end.min(text::line_end(rope, text::line_of(rope, at)))
                };
                if range.is_empty() {
                    return Step::nothing();
                }
                let taken = rope.byte_slice(range.clone()).to_string();
                self.registers.delete(register, Contents::charwise(taken));
                Step::Consumed(vec![
                    Effect::Replace(vec![(range.clone(), String::new())]),
                    Effect::Select(vec![Selection::caret(range.start)]),
                ])
            }
            "delete_to_line_end" | "change_to_line_end" => {
                let register = self.take_register();
                let end = text::line_end(rope, text::line_of(rope, at));
                let range = at..end;
                let taken = rope.byte_slice(range.clone()).to_string();
                self.registers.delete(register, Contents::charwise(taken));
                let change = action.leaf().starts_with("change");
                let mut effects = vec![
                    Effect::Replace(vec![(range, String::new())]),
                    Effect::Select(vec![Selection::caret(at)]),
                ];
                if change {
                    self.mode = Mode::Insert;
                    effects.push(Effect::Mode(Mode::Insert));
                }
                Step::Consumed(effects)
            }
            "yank_line" => {
                let register = self.take_register();
                let line = text::line_of(rope, at);
                let last = rope.len_lines().saturating_sub(1);
                let to = (line + count.max(1) as usize - 1).min(last);
                let range = text::linewise_range(rope, line, to);
                let taken = rope.byte_slice(range.clone()).to_string();
                self.registers.yank(register, Contents::linewise(taken));
                Step::one(Effect::Flash(vec![range]))
            }
            "replace_char" => {
                self.awaiting = Some(Awaiting::ReplaceChar { count });
                Step::Pending
            }
            "toggle_case_char" => {
                let mut end = at;
                for _ in 0..count {
                    end = text::next_grapheme(rope, end);
                }
                let end = end.min(text::line_end(rope, text::line_of(rope, at)));
                if end <= at {
                    return Step::nothing();
                }
                let text = rope.byte_slice(at..end).to_string();
                Step::Consumed(vec![
                    Effect::Replace(vec![(at..end, swap_case(&text))]),
                    Effect::Select(vec![Selection::caret(text::clamp_normal(rope, end))]),
                ])
            }
            "join_lines" => self.join(count, action.args.flag("keep_spaces"), cx),
            "paste" => self.paste(action, count, cx),
            "repeat" => self.repeat(),
            other => Step::one(Effect::Complain(format!("no edit {other}"))),
        }
    }

    /// `J`: the next line joined onto this one, `count` lines at a time.
    ///
    /// The break and the blanks that start the next line give way to one space. There is no space
    /// when this line already ends in one, when the next line is empty, or when this one is. Every
    /// join is computed against the text as it is now, so they all go in as one change.
    pub(super) fn join(&mut self, count: u32, keep_spaces: bool, cx: &Context<'_>) -> Step {
        let rope = cx.rope;
        let line = text::line_of(rope, self.caret(cx));
        let last = rope.len_lines().saturating_sub(1);
        // `J` with no count joins two lines; with one, that many.
        let joins = count.max(2) as usize - 1;

        let mut replacements = Vec::new();
        let mut caret = self.caret(cx);
        let mut moved = 0isize;

        for step in 0..joins {
            let here = line + step;
            if here >= last {
                break;
            }
            let start = text::line_start(rope, here);
            let end = text::line_end(rope, here);
            let next_first = text::first_non_blank(rope, here + 1);
            let next_end = text::line_end(rope, here + 1);

            let (to, filler) = if keep_spaces {
                (text::line_start(rope, here + 1), String::new())
            } else {
                let ends_blank = end > start
                    && text::char_at(rope, text::prev_grapheme(rope, end))
                        .is_some_and(char::is_whitespace);
                let filler = if ends_blank || next_first == next_end || end == start {
                    String::new()
                } else {
                    " ".to_owned()
                };
                (next_first, filler)
            };

            // Where the caret ends up, in the text as it will be: on the join, which for several
            // joins is the last one.
            caret = (end as isize + moved) as usize;
            moved += filler.len() as isize - (to - end) as isize;
            replacements.push((end..to, filler));
        }

        if replacements.is_empty() {
            return Step::nothing();
        }
        Step::Consumed(vec![
            Effect::Replace(replacements),
            Effect::Select(vec![Selection::caret(caret)]),
        ])
    }

    /// `p` and `P`.
    pub(super) fn paste(&mut self, action: &Action, count: u32, cx: &Context<'_>) -> Step {
        let rope = cx.rope;
        let at = self.caret(cx);
        let before = action.args.flag("before");
        let register = self.take_register();

        if register.is_clipboard() {
            return Step::one(Effect::ReadClipboard {
                primary: register.character() == '*',
                before,
            });
        }

        let contents = self.registers.get(register);
        if contents.text.is_empty() {
            return Step::nothing();
        }
        let text = contents.text.repeat(count.max(1) as usize);

        if contents.linewise {
            let line = text::line_of(rope, at);
            let at = if before {
                text::line_start(rope, line)
            } else if line + 1 < rope.len_lines() {
                text::line_start(rope, line + 1)
            } else {
                // Below the last line: the break comes first instead of last.
                let end = rope.len_bytes();
                let text = format!("\n{}", text.trim_end_matches('\n'));
                let caret = end + 1;
                return Step::Consumed(vec![
                    Effect::Replace(vec![(end..end, text)]),
                    Effect::Select(vec![Selection::caret(caret)]),
                    Effect::Scroll(Scroll::EnsureVisible),
                ]);
            };
            // The caret lands on the first non-blank of what was pasted.
            let leading = text.len() - text.trim_start_matches([' ', '\t']).len();
            return Step::Consumed(vec![
                Effect::Replace(vec![(at..at, text)]),
                Effect::Select(vec![Selection::caret(at + leading)]),
                Effect::Scroll(Scroll::EnsureVisible),
            ]);
        }

        let at = if before {
            at
        } else {
            text::next_grapheme(rope, at)
                .min(text::line_end(rope, text::line_of(rope, at)))
                .max(at)
        };
        let caret = at + text.len();
        Step::Consumed(vec![
            Effect::Replace(vec![(at..at, text)]),
            // On the last character of what was pasted, which is where vim leaves it.
            Effect::Select(vec![Selection::caret(caret.saturating_sub(1).max(at))]),
            Effect::Scroll(Scroll::EnsureVisible),
        ])
    }

    /// `.`, which puts the last change back by replaying the keys that made it.
    pub(super) fn repeat(&mut self) -> Step {
        match self.last_change.as_ref() {
            Some(repeat) => Step::one(Effect::App(Action::with(
                "vim.replay",
                Args::new(
                    [(
                        "keys".to_owned(),
                        toml::Value::String(crate::notation::format(&repeat.keys)),
                    )]
                    .into_iter()
                    .collect(),
                ),
            ))),
            None => Step::nothing(),
        }
    }
}
