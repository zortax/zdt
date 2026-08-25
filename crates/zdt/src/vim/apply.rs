//! Turning what the engine answers into editor commands.

use zgui_terminal::SpanKind;

use super::*;

/// How long a yank stays lit.
const FLASH: std::time::Duration = std::time::Duration::from_millis(200);

/// The decoration layer a flash is drawn in.
const FLASH_LAYER: &str = "vim-flash";

/// Puts `text` on the desktop's clipboard.
///
/// For a surface that does not hold the clipboards itself. An editor does, and copies what it has
/// selected without being told the text.
fn set_clipboard(text: &str, primary: bool) {
    if let Some(clipboards) = zgui::prelude::try_use_clipboard() {
        clipboards.set_text(kind_of(primary), text.to_owned());
    }
}

/// Reads the desktop's clipboard and hands what comes back to `then`.
fn read_clipboard(primary: bool, then: impl FnOnce(String) + 'static) {
    if let Some(clipboards) = zgui::prelude::try_use_clipboard() {
        clipboards.read_text(kind_of(primary), move |text| {
            if let Some(text) = text {
                then(text);
            }
        });
    }
}

/// Which of the desktop's two clipboards is meant.
fn kind_of(primary: bool) -> zgui::prelude::ClipboardKind {
    if primary {
        zgui::prelude::ClipboardKind::Primary
    } else {
        zgui::prelude::ClipboardKind::Standard
    }
}

/// What the editor paints for a visual selection.
///
/// The caret goes where vim puts it, inside what is selected rather than after it. A block also
/// hands over its rectangle, because the bytes on a line too short to reach the right edge do not
/// describe the shape the person is drawing.
fn overlay(visual: &Option<Visual>) -> Overlay {
    let Some(visual) = visual else {
        return Overlay::default();
    };
    Overlay {
        bands: visual
            .lines
            .clone()
            .map(|line| Band {
                line,
                columns: visual.columns.start as u32..visual.columns.end as u32,
            })
            .collect(),
        carets: vec![Caret {
            line: visual.line,
            column: visual.column as u32,
        }],
    }
}

impl Vim {
    /// One key, without publishing what changed.
    pub(super) fn step(&self, chord: Chord, surface: Surface<'_>) -> Step {
        let engine = &self.inner.engine;
        let owner = self.owner_of(surface);
        self.inner
            .keymaps
            .with_layered(surface.region(), |layered| {
                surface.query(owner, |context| {
                    engine.borrow_mut().key(chord, layered, context)
                })
            })
    }

    /// Which buffer `surface` is, as the engine names buffers.
    ///
    /// A terminal says so itself: a float is not the buffer the panes are showing, and a mark set
    /// in one must not name the file underneath it.
    pub(super) fn owner_of(&self, surface: Surface<'_>) -> zdt_vim::Owner {
        use slotmap::Key;

        match surface.scrollback() {
            Some(scrollback) => zdt_vim::Owner(scrollback.buffer().data().as_ffi()),
            None => self.owner(),
        }
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
    pub(super) fn carry_out(&self, effects: Vec<Effect>, surface: Surface<'_>) {
        match surface {
            Surface::Editor(handle) => {
                for effect in effects {
                    self.to_editor(effect, handle);
                }
            }
            Surface::Terminal(scrollback) => {
                for effect in effects {
                    self.to_terminal(effect, scrollback);
                }
            }
        }
    }

    /// One effect, in an editor.
    fn to_editor(&self, effect: Effect, handle: &EditorHandle) {
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
            Effect::Visual(visual) => {
                handle.set_overlay(overlay(&visual));
            }
            Effect::Flash(ranges) => self.flash(ranges, handle),
            Effect::Replace(replacements) => {
                handle.command(Command::ReplaceRanges(replacements));
            }
            Effect::GoTo(place) => self.go_to(place),
            Effect::Undo => handle.command(Command::Undo),
            Effect::Redo => handle.command(Command::Redo),
            Effect::Scroll(scroll) => handle.command(Command::Scroll(match scroll {
                Scroll::Center => ScrollCmd::CursorCenter,
                Scroll::Top => ScrollCmd::CursorTop,
                Scroll::Bottom => ScrollCmd::CursorBottom,
                Scroll::Lines(lines) => ScrollCmd::Lines(f64::from(lines)),
                Scroll::EnsureVisible => ScrollCmd::EnsureCursorVisible,
            })),
            Effect::Mode(mode) => {
                if !mode.is_visual() {
                    // Nothing is selected any more, so the selections paint themselves again.
                    handle.set_overlay(Overlay::default());
                }
                self.enter(mode, handle);
            }
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
            Effect::App(action) => self.app_action(&action, Surface::Editor(handle)),
            Effect::Say(text) => self.inner.workspace.say(text),
            Effect::Complain(text) => self.inner.workspace.complain(text),
        }
    }

    /// One effect, over what a terminal holds.
    ///
    /// A terminal takes text and never gives any back. An insertion is sent to the program, which
    /// is what makes `p` paste into a shell; anything that would take away what is on the screen
    /// is refused, because the screen is the program's to write.
    fn to_terminal(&self, effect: Effect, scrollback: &Scrollback) {
        match effect {
            Effect::Select(selections) => scrollback.select(&selections),
            Effect::Visual(None) => scrollback.unpaint(),
            Effect::Visual(Some(visual)) => {
                // Which shape the grid paints is which visual mode this is, and the engine is
                // the one that knows. It has already taken the mode change by now: entering one
                // says so before it says what is selected.
                let kind = match self.inner.engine.borrow().mode() {
                    Mode::VisualLine => SpanKind::Lines,
                    Mode::VisualBlock => SpanKind::Block,
                    _ => SpanKind::Cells,
                };
                scrollback.paint(&visual, kind);
            }
            Effect::Flash(ranges) => self.flash_terminal(&ranges, scrollback),
            Effect::Replace(replacements) => {
                if replacements.iter().any(|(range, _)| !range.is_empty()) {
                    self.inner.workspace.complain("a terminal cannot be edited");
                    return;
                }
                let text: String = replacements.into_iter().map(|(_, text)| text).collect();
                scrollback.insert(&text);
            }
            Effect::Undo | Effect::Redo => {
                self.inner.workspace.complain("a terminal cannot be edited");
            }
            Effect::Scroll(scroll) => scrollback.scroll(scroll),
            Effect::Mode(mode) => {
                if !mode.is_visual() {
                    scrollback.unpaint();
                }
                // A terminal draws its own block wherever the caret is, in every mode it has. It
                // has no gutter and nothing to number.
                scrollback.show_cursor();
            }
            // The engine read the text out for itself, so what is on the clipboard is what it
            // took. A terminal has no clipboard commands of its own to defer to.
            Effect::SetClipboard { text, primary } => set_clipboard(&text, primary),
            Effect::ReadClipboard { primary, .. } => {
                let scrollback = scrollback.clone();
                read_clipboard(primary, move |text| scrollback.insert(&text));
            }
            Effect::GoTo(place) => self.go_to(place),
            Effect::App(action) => self.app_action(&action, Surface::Terminal(scrollback)),
            Effect::Say(text) => self.inner.workspace.say(text),
            Effect::Complain(text) => self.inner.workspace.complain(text),
        }
    }

    /// Lights what a command took on the grid, and puts it out again a moment later.
    fn flash_terminal(&self, ranges: &[std::ops::Range<usize>], scrollback: &Scrollback) {
        if !scrollback.flash(ranges) {
            return;
        }
        let Some(timers) = zgui::view::time::Timers::current() else {
            return;
        };
        let scrollback = scrollback.clone();
        let waiting = timers.set_timeout(FLASH, move || scrollback.unflash());
        *self.inner.flash.borrow_mut() = Some(waiting);
    }

    /// Lights `ranges` in the selection colour, and puts them out again a moment later.
    ///
    /// The handle for the timer is kept, so a second yank inside the moment cancels the first
    /// one's clearing rather than being cut short by it.
    fn flash(&self, ranges: Vec<std::ops::Range<usize>>, handle: &EditorHandle) {
        let decorations: Vec<Decoration> = ranges
            .into_iter()
            .filter(|range| !range.is_empty())
            .map(|range| Decoration::background(range, "editor-selection"))
            .collect();
        if decorations.is_empty() {
            return;
        }
        handle.set_decorations(FLASH_LAYER, decorations);

        let Some(timers) = zgui::view::time::Timers::current() else {
            return;
        };
        let editor = handle.clone();
        let waiting = timers.set_timeout(FLASH, move || editor.clear_decorations(FLASH_LAYER));
        *self.inner.flash.borrow_mut() = Some(waiting);
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
        let mode = self.inner.engine.borrow().mode();
        self.enter(mode, handle);
    }

    /// An action the engine handed back because the application owns it.
    fn app_action(&self, action: &zdt_vim::Action, surface: Surface<'_>) {
        if action.name == "vim.replay" {
            let keys = action.args.str("keys").unwrap_or_default().to_owned();
            self.replay(&keys, surface);
            return;
        }
        crate::actions::run(&self.inner.workspace, self, action, surface.editor());
    }

    /// Plays `keys` as though they had been typed.
    ///
    /// One key at a time, with the editor read again between them. A macro that edits and then
    /// moves needs the text as it is after the edit.
    pub fn replay(&self, keys: &str, surface: Surface<'_>) {
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
            let step = self.step(chord, surface);
            match step {
                Step::Consumed(effects) => self.carry_out(effects, surface),
                Step::Pending => {}
                // A key the engine did not want, replayed: it is text, and the editor's default
                // handling is what typing it would have done.
                Step::PassThrough => {
                    if let Some(character) = chord.inserted()
                        && let Some(handle) = surface.editor()
                    {
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
