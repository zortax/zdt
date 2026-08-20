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

        // A block's caret keeps a column of its own, past the end of a short line. Moving sideways
        // moves it, moving up and down keeps it, and any other motion gives it up to wherever it
        // landed. It is the goal column, which up and down already aim at.
        if self.mode == Mode::VisualBlock {
            let column = self.block_column(from, cx);
            self.goal_column = match action.leaf() {
                "right" => Some(column + count as usize),
                "left" => Some(column.saturating_sub(count as usize)),
                "down" | "up" => Some(column),
                _ => None,
            };
        }

        let mut target = match action.leaf() {
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

        // Sideways in a block, the byte follows the column rather than the column the byte: a
        // column past the end of a line has no byte of its own, and the line's end is where the
        // caret must sit on the buffer.
        if self.mode == Mode::VisualBlock && matches!(action.leaf(), "left" | "right") {
            let line = text::line_of(rope, from);
            let column = self.goal_column.unwrap_or_default();
            target = Target::exclusive(motion::byte_at_column(rope, line, column));
        }

        // Only the two vertical motions keep a goal column; everything else sets a new one. A
        // block has already decided, above.
        if self.mode != Mode::VisualBlock {
            if !matches!(action.leaf(), "down" | "up") {
                self.goal_column = None;
            } else if self.goal_column.is_none() {
                self.goal_column = Some(motion::column_of(rope, from));
            }
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
            self.jumps.push(cx.place());
        }

        let byte = text::clamp_normal(cx.rope, target.byte);
        if self.mode.is_visual() {
            self.visual_head = byte;
        }

        let mut effects = self.place(byte, cx);
        effects.push(Effect::Scroll(Scroll::EnsureVisible));
        Step::Consumed(effects)
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
            Mode::VisualBlock => {
                let (lines, columns) = self.block_extent(head, cx);
                (
                    block_selections(rope, lines, columns)
                        .into_iter()
                        .map(Selection::range)
                        .filter(|range| !range.is_empty())
                        .collect(),
                    false,
                )
            }
            _ => {
                let (start, end) = (anchor.min(head), anchor.max(head));
                (
                    std::iter::once(start..text::next_grapheme(rope, end)).collect(),
                    false,
                )
            }
        }
    }

    /// The rectangle a block selection covers: its lines, and the columns on each of them.
    ///
    /// The caret's column is the one it moved to, which may be past the end of its own line. That
    /// is what keeps the rectangle a rectangle over lines too short to reach it.
    pub(super) fn block_extent(
        &self,
        byte: usize,
        cx: &Context<'_>,
    ) -> (std::ops::Range<usize>, std::ops::Range<usize>) {
        let rope = cx.rope;
        let anchor = self.anchor_column(cx);
        let head = self.block_column(byte, cx);
        let (one, two) = (
            text::line_of(rope, self.visual_anchor),
            text::line_of(rope, byte),
        );
        (
            one.min(two)..one.max(two) + 1,
            anchor.min(head)..anchor.max(head) + 1,
        )
    }

    /// The column a block's caret is at, which the goal column carries past the end of a line.
    pub(super) fn block_column(&self, byte: usize, cx: &Context<'_>) -> usize {
        self.goal_column
            .unwrap_or_else(|| motion::column_of(cx.rope, byte))
    }

    /// The column a block's other corner is at.
    pub(super) fn anchor_column(&self, cx: &Context<'_>) -> usize {
        self.visual_anchor_column
            .unwrap_or_else(|| motion::column_of(cx.rope, self.visual_anchor))
    }

    /// What to do once the caret has moved to `byte`: which bytes are selected, and how they
    /// paint.
    ///
    /// The two are not the same thing in a visual mode. The bytes are what an operator takes and
    /// what a copy puts on the clipboard; the paint is where the caret sits and which cells read
    /// as selected, and vim puts the caret inside what it selected rather than after it.
    pub(super) fn place(&self, byte: usize, cx: &Context<'_>) -> Vec<Effect> {
        let rope = cx.rope;
        let (line, column) = (text::line_of(rope, byte), motion::column_of(rope, byte));

        match self.mode {
            Mode::Visual | Mode::Select => {
                // Through the character the caret is on, which is what makes `vy` take one
                // character rather than none.
                let anchor = self.visual_anchor;
                let selection = if byte >= anchor {
                    Selection::new(anchor, text::next_grapheme(rope, byte))
                } else {
                    Selection::new(text::next_grapheme(rope, anchor), byte)
                };
                vec![
                    Effect::Select(vec![selection]),
                    Effect::Visual(Some(Visual::at(line, column))),
                ]
            }
            Mode::VisualLine => {
                let from = text::line_of(rope, self.visual_anchor);
                let range = text::linewise_range(rope, from, line);
                vec![
                    Effect::Select(vec![Selection::new(range.start, range.end)]),
                    Effect::Visual(Some(Visual::at(line, column))),
                ]
            }
            Mode::VisualBlock => {
                let (lines, columns) = self.block_extent(byte, cx);
                let selections = block_selections(rope, lines.clone(), columns.clone());
                vec![
                    Effect::Select(selections),
                    Effect::Visual(Some(Visual {
                        line,
                        column: self.block_column(byte, cx),
                        lines,
                        columns,
                    })),
                ]
            }
            _ => vec![Effect::Select(vec![Selection::caret(byte)])],
        }
    }
}
