//! Marks, the jump list, and macros.

use super::*;

impl Engine {
    /// `m` and the two ways of jumping to a mark.
    pub(super) fn mark(&mut self, action: &Action) -> Step {
        match action.leaf() {
            "set" => {
                self.awaiting = Some(Awaiting::SetMark);
                Step::Pending
            }
            "jump" => {
                self.awaiting = Some(Awaiting::JumpMark {
                    line: action.args.flag("line"),
                });
                Step::Pending
            }
            other => Step::one(Effect::Complain(format!("no mark command {other}"))),
        }
    }

    /// The jump list and the change list.
    pub(super) fn jump(&mut self, action: &Action, cx: &Context<'_>) -> Step {
        let target = match action.leaf() {
            "back" => self.jumps.back(cx.cursor()),
            "forward" => self.jumps.forward(),
            // The change list is the application's, because it is the editor that holds the
            // history the changes are in.
            "older_change" | "newer_change" => return Step::one(Effect::App(action.clone())),
            other => return Step::one(Effect::Complain(format!("no jump {other}"))),
        };
        match target {
            Some(byte) => Step::Consumed(vec![
                Effect::Select(vec![Selection::caret(text::clamp_normal(cx.rope, byte))]),
                Effect::Scroll(Scroll::EnsureVisible),
            ]),
            None => Step::nothing(),
        }
    }

    /// `q` and `@`.
    pub(super) fn macro_action(&mut self, action: &Action) -> Step {
        match action.leaf() {
            "record" => {
                if let Some((name, mut keys)) = self.recording_macro.take() {
                    // The `q` that stopped it is not part of it.
                    keys.pop();
                    self.registers
                        .set_quietly(name, Contents::charwise(crate::notation::format(&keys)));
                    return Step::one(Effect::Say(format!("recorded @{name}")));
                }
                self.awaiting = Some(Awaiting::RecordMacro);
                Step::Pending
            }
            "play" => {
                self.awaiting = Some(Awaiting::PlayMacro);
                Step::Pending
            }
            other => Step::one(Effect::Complain(format!("no macro command {other}"))),
        }
    }

    /// Runs `keys` as though they had been typed, for `.` and for macros.
    ///
    /// The context is read again between keys by the caller, which is what makes a macro that
    /// edits and then moves work.
    pub fn begin_replay(&mut self) {
        self.replaying = true;
        self.macro_depth += 1;
    }

    /// Ends what [`begin_replay`](Self::begin_replay) started.
    pub fn end_replay(&mut self) {
        self.replaying = false;
        self.macro_depth = self.macro_depth.saturating_sub(1);
    }
}
