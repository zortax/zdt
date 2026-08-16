//! Where a motion puts the caret, and what a selection covers.

use super::*;

impl Engine {
    /// Where the caret is: the visual head in a visual mode, the primary caret otherwise.
    pub(super) fn caret(&self, cx: &Context<'_>) -> usize {
        if self.mode.is_visual() {
            self.visual_head
        } else {
            cx.cursor()
        }
    }

    /// Where a motion goes, and what to do about having gone there.
    pub(super) fn motion(&mut self, action: &Action, count: u32, cx: &Context<'_>) -> Step {
        let rope = cx.rope;
        let from = self.caret(cx);
        let args = &action.args;

        let target = match action.leaf() {
            "left" => motion::left(rope, from, count),
            "right" => motion::right(rope, from, count),
            "down" => motion::down(rope, from, count, self.goal_column),
            "up" => motion::up(rope, from, count, self.goal_column),
            "word_forward" => motion::word_forward(rope, from, count, args.flag("big")),
            "word_backward" => motion::word_backward(rope, from, count, args.flag("big")),
            "word_end" if args.flag("backward") => {
                motion::word_end_backward(rope, from, count, args.flag("big"))
            }
            "word_end" => motion::word_end(rope, from, count, args.flag("big")),
            "line_start" => motion::line_start(rope, from),
            "first_non_blank" => motion::first_non_blank(rope, from),
            "line_end" => motion::line_end(rope, from, count),
            "last_non_blank" => motion::last_non_blank(rope, from, count),
            "document_start" => motion::goto_line(
                rope,
                self.count.take().or(Some(count)).filter(|_| count > 1),
            ),
            "document_end" => motion::document_end(rope, (count > 1).then_some(count)),
            "paragraph_forward" => motion::paragraph_forward(rope, from, count),
            "paragraph_backward" => motion::paragraph_backward(rope, from, count),
            "matching_bracket" => match motion::matching_bracket(rope, from) {
                Some(target) => target,
                None => return Step::nothing(),
            },
            "screen_top" => motion::screen_top(rope, cx.view, count),
            "screen_middle" => motion::screen_middle(rope, cx.view),
            "screen_bottom" => motion::screen_bottom(rope, cx.view, count),
            "half_page_down" => motion::half_page_down(rope, from, cx.view, count),
            "half_page_up" => motion::half_page_up(rope, from, cx.view, count),
            "page_down" => motion::page_down(rope, from, cx.view, count),
            "page_up" => motion::page_up(rope, from, cx.view, count),
            "find_char" => {
                self.awaiting = Some(Awaiting::FindChar {
                    backward: args.flag("backward"),
                    till: args.flag("till"),
                    count,
                });
                return Step::Pending;
            }
            "repeat_find" => {
                let Some(find) = self.last_find else {
                    return Step::nothing();
                };
                let find = if args.flag("reverse") {
                    find.reversed()
                } else {
                    find
                };
                match motion::find_char(rope, from, count, find, true) {
                    Some(target) => target,
                    None => return Step::nothing(),
                }
            }
            _ => return Step::one(Effect::Complain(format!("no motion {}", action.name))),
        };

        // Only the two vertical motions keep a goal column; everything else sets a new one.
        if !matches!(action.leaf(), "down" | "up") {
            self.goal_column = None;
        } else if self.goal_column.is_none() {
            self.goal_column = Some(motion::column_of(rope, from));
        }

        self.go(target, cx)
    }

    /// Puts the caret at `byte`, or hands the range there to a waiting operator.
    ///
    /// What a leap comes back as. The three keystrokes that chose it belong to the layer that
    /// drew the labels. Where they landed is a motion like any other, which is what makes
    /// `ds{ab}` delete up to it.
    ///
    /// Exclusive, as leap.nvim's is. `ds` up to a pair takes what is before it and leaves the pair
    /// itself. It is a jump, so `<C-o>` comes back.
    pub fn leap_to(&mut self, byte: usize, cx: &Context<'_>) -> Step {
        let mut target = Target::exclusive(byte);
        target.jump = true;
        self.go(target, cx)
    }

    /// Moves the caret to `target`, or hands the range to a waiting operator.
    pub(super) fn go(&mut self, target: Target, cx: &Context<'_>) -> Step {
        if let Some(pending) = self.operator.take() {
            let range = operator_range(cx.rope, cx.cursor(), target);
            return self.apply_operator(&pending.action, range, target.kind == Kind::Linewise, cx);
        }

        if target.jump {
            self.jumps.push(self.caret(cx));
        }

        let byte = text::clamp_normal(cx.rope, target.byte);
        if self.mode.is_visual() {
            self.visual_head = byte;
        }

        Step::Consumed(vec![
            Effect::Select(self.selections_for(byte, cx)),
            Effect::Scroll(Scroll::EnsureVisible),
        ])
    }

    /// What a visual selection covers, and whether it is by lines.
    ///
    /// Charwise is inclusive of the character the caret is on, which is the whole difference
    /// between `vld` taking two characters and taking one.
    pub(super) fn visual_ranges(&self, cx: &Context<'_>) -> (Vec<std::ops::Range<usize>>, bool) {
        let rope = cx.rope;
        let (anchor, head) = (self.visual_anchor, self.visual_head);
        match self.mode {
            Mode::VisualLine => {
                let (from, to) = (text::line_of(rope, anchor), text::line_of(rope, head));
                (vec![text::linewise_range(rope, from, to)], true)
            }
            Mode::VisualBlock => (
                block_selections(rope, anchor, head)
                    .into_iter()
                    .map(Selection::range)
                    .filter(|range| !range.is_empty())
                    .collect(),
                false,
            ),
            _ => {
                let (start, end) = (anchor.min(head), anchor.max(head));
                (
                    std::iter::once(start..text::next_grapheme(rope, end)).collect(),
                    false,
                )
            }
        }
    }

    /// The selections after the caret moves to `byte`.
    pub(super) fn selections_for(&self, byte: usize, cx: &Context<'_>) -> Vec<Selection> {
        match self.mode {
            Mode::Visual | Mode::Select => vec![Selection::new(self.visual_anchor, byte)],
            Mode::VisualLine => {
                let rope = cx.rope;
                let (from, to) = (
                    text::line_of(rope, self.visual_anchor),
                    text::line_of(rope, byte),
                );
                let range = text::linewise_range(rope, from, to);
                // The head end is the one the caret is on, so `o` and further motion work.
                if byte >= self.visual_anchor {
                    vec![Selection::new(range.start, range.end)]
                } else {
                    vec![Selection::new(range.end, range.start)]
                }
            }
            Mode::VisualBlock => block_selections(cx.rope, self.visual_anchor, byte),
            _ => vec![Selection::caret(byte)],
        }
    }
}
