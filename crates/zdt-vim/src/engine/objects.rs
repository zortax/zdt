//! What `iw` and `ap` select.

use super::*;

impl Engine {
    /// What a text object selects, and what to do with it.
    pub(super) fn text_object(&mut self, action: &Action, _count: u32, cx: &Context<'_>) -> Step {
        let rope = cx.rope;
        let at = self.caret(cx);
        let args = &action.args;
        let around = args.flag("around");

        let range = match action.leaf() {
            "word" => textobject::word(rope, at, args.flag("big"), around),
            "paragraph" => textobject::paragraph(rope, at, around),
            "sentence" => textobject::sentence(rope, at, around),
            "quote" => args
                .char("quote")
                .and_then(|quote| textobject::quote(rope, at, quote, around)),
            "pair" => args
                .char("open")
                .and_then(|open| textobject::pair(rope, at, open, around)),
            // Tree-sitter's, which the application answers because it owns the parser.
            "function" | "class" => return Step::one(Effect::App(action.clone())),
            _ => return Step::one(Effect::Complain(format!("no text object {}", action.name))),
        };

        let Some(range) = range else {
            // Nothing to select. The operator stays pending, the way vim leaves it.
            return Step::nothing();
        };

        if let Some(pending) = self.operator.take() {
            return self.apply_operator(&pending.action, range, false, cx);
        }
        if self.mode.is_visual() {
            self.visual_anchor = range.start;
            self.visual_head = text::prev_grapheme(rope, range.end);
            self.leave_visual();
            return Step::Consumed(self.place(self.visual_head, cx));
        }
        Step::nothing()
    }
}
