//! What an operator does to the range a motion gave it.

use super::*;

impl Engine {
    /// An operator: either applied to the selection, or waiting for what to apply to.
    pub(super) fn operator(&mut self, action: &Action, count: u32, cx: &Context<'_>) -> Step {
        if self.mode.is_visual() {
            let (ranges, linewise) = self.visual_ranges(cx);
            self.mode = Mode::Normal;
            self.leave_visual();
            return self.apply_ranges(action, ranges, linewise, cx);
        }

        // Already pending and typed again means "this line", which the caller handles; reaching
        // here with one pending is a second, different operator, and the first is abandoned.
        self.operator = Some(PendingOperator {
            action: action.clone(),
            key: self.resolved,
            count,
        });
        self.mode = Mode::OperatorPending;
        Step::one(Effect::Mode(Mode::OperatorPending))
    }

    /// The operator, applied to whole lines from the caret.
    pub(super) fn apply_linewise_operator(
        &mut self,
        action: &Action,
        count: u32,
        cx: &Context<'_>,
    ) -> Step {
        let rope = cx.rope;
        let line = text::line_of(rope, cx.cursor());
        let last = rope.len_lines().saturating_sub(1);
        let to = (line + count.max(1) as usize - 1).min(last);
        let range = text::linewise_range(rope, line, to);
        self.apply_operator(action, range, true, cx)
    }

    /// The operator, applied to one range.
    pub(super) fn apply_operator(
        &mut self,
        action: &Action,
        range: std::ops::Range<usize>,
        linewise: bool,
        cx: &Context<'_>,
    ) -> Step {
        self.apply_ranges(action, vec![range], linewise, cx)
    }

    /// The operator, applied to every range it was given.
    fn apply_ranges(
        &mut self,
        action: &Action,
        ranges: Vec<std::ops::Range<usize>>,
        linewise: bool,
        cx: &Context<'_>,
    ) -> Step {
        let rope = cx.rope;
        let register = self.take_register();
        let mut effects = Vec::new();

        let ranges: Vec<std::ops::Range<usize>> = ranges
            .into_iter()
            .filter(|range| !range.is_empty() || linewise)
            .collect();

        if ranges.is_empty() {
            self.mode = Mode::Normal;
            return Step::one(Effect::Mode(Mode::Normal));
        }

        let taken: String = ranges
            .iter()
            .map(|range| rope.byte_slice(range.clone()).to_string())
            .collect::<Vec<_>>()
            .join("");

        let contents = if linewise {
            Contents::linewise(taken.clone())
        } else {
            Contents::charwise(taken.clone())
        };

        let first = ranges.first().expect("not empty").start;

        match action.leaf() {
            "yank" => {
                self.registers.yank(register, contents);
                // A yank leaves the text as it was, so lighting what it took is the only sign
                // that it took anything.
                effects.push(Effect::Flash(ranges.clone()));
                if register.is_clipboard() {
                    effects.push(Effect::SetClipboard {
                        text: taken,
                        primary: register.character() == '*',
                    });
                }
                // The caret goes to the start of what was yanked, which is what vim does.
                effects.push(Effect::Select(vec![Selection::caret(text::clamp_normal(
                    rope, first,
                ))]));
                self.mode = Mode::Normal;
                effects.push(Effect::Mode(Mode::Normal));
            }
            "delete" | "change" => {
                self.registers.delete(register, contents);
                if register.is_clipboard() {
                    effects.push(Effect::SetClipboard {
                        text: taken,
                        primary: register.character() == '*',
                    });
                }
                let change = action.leaf() == "change";
                if change && linewise {
                    // `cc` empties the line and leaves the caret on it. The line stays.
                    let line = text::line_of(rope, first);
                    let indent = rope
                        .byte_slice(text::line_start(rope, line)..text::first_non_blank(rope, line))
                        .to_string();
                    effects.push(Effect::Replace(
                        ranges
                            .iter()
                            .map(|range| (range.clone(), indent.clone() + "\n"))
                            .collect(),
                    ));
                    effects.push(Effect::Select(vec![Selection::caret(first + indent.len())]));
                } else {
                    let caret = if linewise {
                        linewise_caret(rope, ranges.first().expect("not empty"))
                    } else {
                        first
                    };
                    effects.push(Effect::Replace(
                        ranges
                            .iter()
                            .map(|range| (range.clone(), String::new()))
                            .collect(),
                    ));
                    effects.push(Effect::Select(vec![Selection::caret(caret)]));
                }
                if change {
                    self.mode = Mode::Insert;
                    effects.push(Effect::Mode(Mode::Insert));
                } else {
                    self.mode = Mode::Normal;
                    effects.push(Effect::Mode(Mode::Normal));
                }
            }
            "indent" | "dedent" => {
                let dedent = action.leaf() == "dedent";
                let replacements = indent_lines(rope, &ranges, dedent);
                // Where the caret lands is measured against the text *after* the change: the
                // first line's own indent has just moved, and the caret goes to its first
                // non-blank.
                let line = text::line_of(rope, first);
                let moved: isize = replacements
                    .iter()
                    .filter(|(range, _)| text::line_of(rope, range.start) == line)
                    .map(|(range, text)| text.len() as isize - (range.end - range.start) as isize)
                    .sum();
                let caret = (text::first_non_blank(rope, line) as isize + moved).max(0) as usize;
                if !replacements.is_empty() {
                    effects.push(Effect::Replace(replacements));
                }
                effects.push(Effect::Select(vec![Selection::caret(caret)]));
                self.mode = Mode::Normal;
                effects.push(Effect::Mode(Mode::Normal));
            }
            "lowercase" | "uppercase" | "swap_case" => {
                let leaf = action.leaf();
                let replacements: Vec<_> = ranges
                    .iter()
                    .map(|range| {
                        let text = rope.byte_slice(range.clone()).to_string();
                        let changed = match leaf {
                            "lowercase" => text.to_lowercase(),
                            "uppercase" => text.to_uppercase(),
                            _ => swap_case(&text),
                        };
                        (range.clone(), changed)
                    })
                    .collect();
                effects.push(Effect::Replace(replacements));
                effects.push(Effect::Select(vec![Selection::caret(text::clamp_normal(
                    rope, first,
                ))]));
                self.mode = Mode::Normal;
                effects.push(Effect::Mode(Mode::Normal));
            }
            // Commenting needs to know the language's comment token, which the application owns.
            "comment" => {
                self.mode = Mode::Normal;
                effects.push(Effect::App(Action::with(
                    action.name.clone(),
                    action.args.clone(),
                )));
                effects.push(Effect::Mode(Mode::Normal));
            }
            other => {
                self.mode = Mode::Normal;
                effects.push(Effect::Complain(format!("no operator {other}")));
            }
        }

        effects.push(Effect::Scroll(Scroll::EnsureVisible));
        Step::Consumed(effects)
    }
}
