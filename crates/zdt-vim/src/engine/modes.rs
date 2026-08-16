//! Going from one mode to another.

use super::*;

impl Engine {
    /// Entering and leaving the modes.
    pub(super) fn mode_change(&mut self, action: &Action, count: u32, cx: &Context<'_>) -> Step {
        let rope = cx.rope;
        let at = cx.cursor();

        match action.leaf() {
            "normal" => {
                let was = self.mode;
                self.operator = None;
                self.awaiting = None;
                self.keys.clear();
                self.count = None;
                self.register = None;
                self.mode = Mode::Normal;
                let mut effects = vec![Effect::Mode(Mode::Normal)];
                // Leaving insert puts the caret back onto a character, which is what makes the
                // block cursor land where vim leaves it.
                if was.is_inserting() || was.is_visual() {
                    let byte = if was.is_inserting() {
                        text::clamp_normal(
                            rope,
                            text::prev_grapheme(rope, at)
                                .max(text::line_start(rope, text::line_of(rope, at))),
                        )
                    } else {
                        text::clamp_normal(rope, at)
                    };
                    effects.push(Effect::Select(vec![Selection::caret(byte)]));
                }
                Step::Consumed(effects)
            }
            "insert" => {
                let byte = match action.args.str("at") {
                    Some("first_non_blank") => text::first_non_blank(rope, text::line_of(rope, at)),
                    Some("after") => text::next_grapheme(rope, at)
                        .min(text::line_end(rope, text::line_of(rope, at))),
                    Some("line_end") => text::line_end(rope, text::line_of(rope, at)),
                    _ => at,
                };
                self.mode = Mode::Insert;
                Step::Consumed(vec![
                    Effect::Select(vec![Selection::caret(byte)]),
                    Effect::Mode(Mode::Insert),
                ])
            }
            "replace" => {
                self.mode = Mode::Replace;
                Step::one(Effect::Mode(Mode::Replace))
            }
            "open_line" => {
                let above = action.args.flag("above");
                let line = text::line_of(rope, at);
                let indent = rope
                    .byte_slice(text::line_start(rope, line)..text::first_non_blank(rope, line))
                    .to_string();
                let (at, text) = if above {
                    let start = text::line_start(rope, line);
                    (start, format!("{indent}\n"))
                } else {
                    let end = text::line_end(rope, line);
                    (end, format!("\n{indent}"))
                };
                let caret = if above {
                    at + indent.len()
                } else {
                    at + 1 + indent.len()
                };
                self.mode = Mode::Insert;
                Step::Consumed(vec![
                    Effect::Replace(vec![(at..at, text)]),
                    Effect::Select(vec![Selection::caret(caret)]),
                    Effect::Mode(Mode::Insert),
                    Effect::Scroll(Scroll::EnsureVisible),
                ])
            }
            "visual" => {
                let kind = match action.args.str("kind") {
                    Some("line") => Mode::VisualLine,
                    Some("block") => Mode::VisualBlock,
                    _ => Mode::Visual,
                };
                if self.mode == kind {
                    // The same visual mode again leaves it, which is what vim does.
                    self.mode = Mode::Normal;
                    return Step::Consumed(vec![
                        Effect::Mode(Mode::Normal),
                        Effect::Select(vec![Selection::caret(text::clamp_normal(rope, at))]),
                    ]);
                }
                if !self.mode.is_visual() {
                    self.visual_anchor = at;
                }
                self.visual_head = at;
                self.mode = kind;
                let _ = count;
                Step::Consumed(vec![
                    Effect::Mode(kind),
                    Effect::Select(self.selections_for(at, cx)),
                ])
            }
            other => Step::one(Effect::Complain(format!("no mode {other}"))),
        }
    }

    /// What only makes sense with something selected.
    pub(super) fn visual(&mut self, action: &Action, cx: &Context<'_>) -> Step {
        match action.leaf() {
            "swap_ends" => {
                std::mem::swap(&mut self.visual_anchor, &mut self.visual_head);
                let head = self.visual_head;
                Step::one(Effect::Select(self.selections_for(head, cx)))
            }
            // A block insert is several carets, which the editor's own multi-caret editing then
            // does the rest of.
            "block_insert" | "block_append" => {
                let append = action.leaf() == "block_append";
                let (ranges, _) = self.visual_ranges(cx);
                let carets: Vec<Selection> = ranges
                    .iter()
                    .map(|range| Selection::caret(if append { range.end } else { range.start }))
                    .collect();
                self.mode = Mode::Insert;
                Step::Consumed(vec![Effect::Select(carets), Effect::Mode(Mode::Insert)])
            }
            other => Step::one(Effect::Complain(format!("no visual command {other}"))),
        }
    }
}
