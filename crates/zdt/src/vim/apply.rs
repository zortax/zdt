//! Turning what the engine answers into editor commands.

use super::*;

impl Vim {
    /// One key, without publishing what changed.
    pub(super) fn step(&self, chord: Chord, handle: &EditorHandle) -> Step {
        let engine = &self.inner.engine;
        let keymap = self.inner.keymap.borrow();
        let layered = Layered::plain(&keymap);

        handle.query(|snapshot| {
            let selections: Vec<Selection> = snapshot
                .selections()
                .iter()
                .map(|selection| Selection::new(selection.anchor, selection.head))
                .collect();
            let visible = snapshot.visible_lines();
            let context = Context {
                rope: snapshot.rope(),
                selections: &selections,
                view: View {
                    top_line: visible.start,
                    height: visible.len().max(1),
                },
            };
            engine.borrow_mut().key(chord, &layered, &context)
        })
    }

    /// Publishes the three things the interface shows.
    pub(super) fn publish(&self) {
        let engine = self.inner.engine.borrow();
        let mode = engine.mode();
        if self.inner.mode.get_untracked() != mode {
            self.inner.mode.set(mode);
        }
        let typed = zdt_vim::notation::format(engine.pending_keys());
        let waiting = engine
            .pending_operator()
            .map(|chord| zdt_vim::notation::format(&[chord]))
            .unwrap_or_default();
        let pending = match engine.pending_count() {
            Some(count) => format!("{count}{waiting}{typed}"),
            None => format!("{waiting}{typed}"),
        };
        if self.inner.pending.get_untracked() != pending {
            self.inner.pending.set(pending);
        }
        let recording = engine.recording();
        if self.inner.recording.get_untracked() != recording {
            self.inner.recording.set(recording);
        }
    }

    /// Does what the engine asked for.
    pub(super) fn carry_out(&self, effects: Vec<Effect>, handle: &EditorHandle) {
        for effect in effects {
            match effect {
                Effect::Select(selections) => {
                    let selections: Vec<zgui_editor::Selection> = selections
                        .iter()
                        .map(|one| zgui_editor::Selection::new(one.anchor, one.head))
                        .collect();
                    if !selections.is_empty() {
                        handle.command(Command::SetSelections {
                            selections,
                            primary: 0,
                        });
                    }
                }
                Effect::Replace(replacements) => {
                    handle.command(Command::ReplaceRanges(replacements));
                }
                Effect::Undo => handle.command(Command::Undo),
                Effect::Redo => handle.command(Command::Redo),
                Effect::Scroll(scroll) => handle.command(Command::Scroll(match scroll {
                    Scroll::Center => ScrollCmd::CursorCenter,
                    Scroll::Top => ScrollCmd::CursorTop,
                    Scroll::Bottom => ScrollCmd::CursorBottom,
                    Scroll::Lines(lines) => ScrollCmd::Lines(f64::from(lines)),
                    Scroll::EnsureVisible => ScrollCmd::EnsureCursorVisible,
                })),
                Effect::Mode(mode) => self.enter(mode, handle),
                Effect::SetClipboard { primary, .. } => {
                    // The editor holds the clipboards and already copies what is selected, so
                    // the engine's own copy stays where it is.
                    handle.command(Command::Copy(if primary {
                        Clipboard::Primary
                    } else {
                        Clipboard::Standard
                    }));
                }
                Effect::ReadClipboard { primary, before } => {
                    if !before {
                        handle.command(Command::Move {
                            motion: zgui_editor::Motion::Right,
                            count: 1,
                            extend: false,
                        });
                    }
                    let _ = InsertPoint::AtCarets;
                    handle.command(Command::Paste(if primary {
                        Clipboard::Primary
                    } else {
                        Clipboard::Standard
                    }));
                }
                Effect::App(action) => self.app_action(&action, handle),
                Effect::Say(text) => self.inner.workspace.say(text),
                Effect::Complain(text) => self.inner.workspace.complain(text),
            }
        }
    }

    /// What the editor looks like in `mode`.
    pub fn enter(&self, mode: Mode, handle: &EditorHandle) {
        use zdt_core::config::LineNumbers;
        use zgui_editor::{CursorStyle, GutterMode};

        handle.set_cursor_style(match mode {
            Mode::Insert | Mode::Command => CursorStyle::Bar,
            Mode::Replace => CursorStyle::Underline,
            _ => CursorStyle::Block,
        });

        // Relative numbering is for moving around; while typing, the absolute number is the one
        // worth having. A person who asked for absolute or for none gets what they asked for in
        // every mode.
        let numbers = self
            .inner
            .settings
            .with_untracked(|config| config.editor.line_numbers);
        handle.set_gutter(match (numbers, mode.is_inserting()) {
            (LineNumbers::None, _) => GutterMode::None,
            (LineNumbers::Absolute, _) | (LineNumbers::Relative, true) => GutterMode::Absolute,
            (LineNumbers::Relative, false) => GutterMode::Relative,
        });
    }

    /// Puts the editor back into whatever the current mode looks like.
    ///
    /// What a settings change calls: the gutter and the caret are decided by the mode and the
    /// settings together, and only one of the two has just moved.
    pub fn refresh(&self, handle: &EditorHandle) {
        self.enter(self.inner.engine.borrow().mode(), handle);
    }

    /// An action the engine handed back because the application owns it.
    fn app_action(&self, action: &zdt_vim::Action, handle: &EditorHandle) {
        if action.name == "vim.replay" {
            let keys = action.args.str("keys").unwrap_or_default().to_owned();
            self.replay(&keys, handle);
            return;
        }
        crate::actions::run(&self.inner.workspace, self, action, Some(handle));
    }

    /// Plays `keys` as though they had been typed.
    ///
    /// One key at a time, with the editor read again between them. A macro that edits and then
    /// moves needs the text as it is after the edit.
    pub fn replay(&self, keys: &str, handle: &EditorHandle) {
        if self.inner.depth.get() >= REPLAY_DEPTH {
            self.inner
                .workspace
                .complain("macros are nested too deeply");
            return;
        }
        let Ok(chords) = parse(keys, Leaders::default()) else {
            return;
        };

        self.inner.depth.set(self.inner.depth.get() + 1);
        self.inner.engine.borrow_mut().begin_replay();
        for chord in chords {
            let step = self.step(chord, handle);
            match step {
                Step::Consumed(effects) => self.carry_out(effects, handle),
                Step::Pending => {}
                // A key the engine did not want, replayed: it is text, and the editor's default
                // handling is what typing it would have done.
                Step::PassThrough => {
                    if let Some(character) = chord.inserted() {
                        handle.command(Command::Insert(character.to_string()));
                    }
                }
            }
        }
        self.inner.engine.borrow_mut().end_replay();
        self.inner
            .depth
            .set(self.inner.depth.get().saturating_sub(1));
        self.publish();
    }
}
