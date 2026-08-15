//! The command line's state, and what its commands do.
//!
//! [`zdt_vim::ex`] reads a line into a description of what was asked for. This is the other half:
//! the text being typed, the history behind it, and the code that carries each description out
//! against the workspace.
//!
//! Everything a command does is something a key can also do. `:w` and `<Leader>w` reach the same
//! save; `:sp` and `<C-w>s` the same split. That is deliberate — two ways to ask for one thing,
//! not two implementations of it.

use std::cell::RefCell;
use std::rc::Rc;

use zdt_vim::ex::{BufferTarget, Command, Range};
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

use crate::workspace::Workspace;

/// How many lines of history to keep.
///
/// A session's worth. This is not a shell and there is no history file to manage.
const HISTORY: usize = 200;

/// The command line.
#[derive(Clone)]
pub struct CommandLine {
    inner: Rc<Inner>,
}

struct Inner {
    workspace: Workspace,
    /// Whether one is being typed.
    open: RwSignal<bool, LocalStorage>,
    /// What has been typed.
    text: RwSignal<String, LocalStorage>,
    /// What was typed before, most recent last.
    history: RefCell<Vec<String>>,
    /// How far back through it the arrows have walked.
    walked: RefCell<Option<usize>>,
}

impl CommandLine {
    /// A command line with nothing being typed.
    #[must_use]
    pub fn new(workspace: Workspace) -> Self {
        Self {
            inner: Rc::new(Inner {
                workspace,
                open: RwSignal::new_local(false),
                text: RwSignal::new_local(String::new()),
                history: RefCell::new(Vec::new()),
                walked: RefCell::new(None),
            }),
        }
    }

    /// Whether one is being typed. Tracked.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.inner.open.get()
    }

    /// What has been typed. Tracked.
    #[must_use]
    pub fn text(&self) -> String {
        self.inner.text.get()
    }

    /// Opens one, starting with `start`.
    ///
    /// `:` opens an empty one; `:'<,'>` is what a visual selection opens, so the range is already
    /// there when the command is typed.
    pub fn open(&self, start: &str) {
        self.inner.text.set(start.to_owned());
        *self.inner.walked.borrow_mut() = None;
        self.inner.open.set(true);
    }

    /// Puts `text` in it, which typing and walking the history both do.
    pub fn set_text(&self, text: &str) {
        if self.inner.text.with_untracked(|held| held != text) {
            self.inner.text.set(text.to_owned());
        }
    }

    /// Closes it without running anything.
    pub fn cancel(&self) {
        self.close();
    }

    /// One step back through the history, if there is one.
    pub fn older(&self) -> Option<String> {
        let history = self.inner.history.borrow();
        if history.is_empty() {
            return None;
        }
        let mut walked = self.inner.walked.borrow_mut();
        let next = match *walked {
            Some(0) => 0,
            Some(at) => at - 1,
            None => history.len() - 1,
        };
        *walked = Some(next);
        history.get(next).cloned()
    }

    /// One step forward, answering nothing at the end — which clears the line.
    pub fn newer(&self) -> Option<String> {
        let history = self.inner.history.borrow();
        let mut walked = self.inner.walked.borrow_mut();
        let at = (*walked)?;
        if at + 1 >= history.len() {
            *walked = None;
            return None;
        }
        *walked = Some(at + 1);
        history.get(at + 1).cloned()
    }

    /// Runs what has been typed, and closes.
    pub fn submit(&self) {
        let line = self.inner.text.get_untracked();
        self.close();

        let trimmed = line.trim();
        if trimmed.is_empty() {
            return;
        }
        {
            let mut history = self.inner.history.borrow_mut();
            // The same command twice in a row is one line of history, not two.
            if history.last().map(String::as_str) != Some(trimmed) {
                history.push(trimmed.to_owned());
                if history.len() > HISTORY {
                    history.remove(0);
                }
            }
        }

        if let Some(command) = zdt_vim::ex::parse(trimmed) {
            self.run(command);
        }
    }

    /// Closes it and gives the keyboard back.
    fn close(&self) {
        if self.inner.open.get_untracked() {
            self.inner.open.set(false);
            self.inner.text.set(String::new());
            self.inner.workspace.focus_editor();
        }
    }

    // ---- Carrying them out --------------------------------------------------------------------

    /// Does what a parsed command asked for.
    pub fn run(&self, command: Command) {
        let workspace = &self.inner.workspace;

        match command {
            Command::Goto(line) => self.goto(line),
            Command::Write {
                path,
                then_quit,
                all,
                ..
            } => {
                self.write(path.as_deref(), all);
                if then_quit {
                    self.quit(false, all);
                }
            }
            Command::Quit { force, all } => self.quit(force, all),
            Command::Edit { path, .. } => match path {
                Some(path) => crate::files::open(workspace, self.resolve(&path)),
                None => workspace.say("re-reading a file is not built yet"),
            },
            Command::BufferDelete { force } => {
                let Some(buffer) = workspace.current_buffer() else {
                    return;
                };
                if buffer.is_dirty() && !force {
                    workspace.complain("unsaved changes; :bd! closes anyway");
                } else {
                    workspace.close_buffer(buffer.id);
                }
            }
            Command::Buffer(target) => self.buffer(target),
            Command::Split { vertical, path } => {
                let axis = if vertical {
                    crate::workspace::Axis::Horizontal
                } else {
                    crate::workspace::Axis::Vertical
                };
                workspace.split(axis);
                if let Some(path) = path {
                    crate::files::open(workspace, self.resolve(&path));
                }
            }
            Command::Substitute {
                range,
                pattern,
                replacement,
                all,
                ignore_case,
            } => self.substitute(&range, &pattern, &replacement, all, ignore_case),
            Command::Set {
                name,
                value,
                off,
                toggle,
            } => self.set(&name, value.as_deref(), off, toggle),
            Command::NoHighlight => workspace.hush(),
            Command::Shell(line) => {
                if let Some(terminals) =
                    zgui::reactive::use_local_context::<crate::terminals::Terminals>()
                {
                    let program = crate::terminals::Program::command(&line);
                    terminals.toggle_float("run", &program);
                }
            }
            Command::Unknown(name) => workspace.complain(format!("no command `{name}`")),
        }
    }

    /// `:42`.
    fn goto(&self, line: usize) {
        let Some(handle) = self.inner.workspace.current_handle() else {
            return;
        };
        let at = handle.query(|snapshot| {
            let rope = snapshot.rope();
            // `usize::MAX` is what `:$` parses to, and clamping is what makes it the last line.
            let line = line
                .saturating_sub(1)
                .min(rope.len_lines().saturating_sub(1));
            rope.char_to_byte(rope.line_to_char(line))
        });
        handle.command(zgui_editor::Command::SetSelections {
            selections: vec![zgui_editor::Selection::caret(at)],
            primary: 0,
        });
        handle.command(zgui_editor::Command::Scroll(
            zgui_editor::ScrollCmd::CursorCenter,
        ));
    }

    /// `:w`, `:wa`, `:w path`.
    fn write(&self, path: Option<&str>, all: bool) {
        let workspace = &self.inner.workspace;
        if all {
            for id in workspace.order() {
                if workspace
                    .buffer_untracked(id)
                    .is_some_and(|buffer| buffer.is_dirty())
                {
                    crate::files::save(workspace, id);
                }
            }
            return;
        }

        let Some(buffer) = workspace.current_buffer() else {
            return;
        };
        match path {
            Some(path) => {
                let Some(document) = buffer.document().cloned() else {
                    return;
                };
                crate::files::save_as(workspace, buffer.id, self.resolve(path), document);
            }
            None => crate::files::save(workspace, buffer.id),
        }
    }

    /// `:q`, `:q!`, `:qa`.
    fn quit(&self, force: bool, all: bool) {
        let workspace = &self.inner.workspace;
        if !all && workspace.close_window() {
            return;
        }

        let unsaved = workspace
            .order()
            .into_iter()
            .filter(|id| {
                workspace
                    .buffer_untracked(*id)
                    .is_some_and(|buffer| buffer.is_dirty())
            })
            .count();
        if unsaved > 0 && !force {
            workspace.complain(format!(
                "{unsaved} buffers have unsaved changes; :q! anyway"
            ));
            return;
        }
        if let Some(windows) =
            zgui::reactive::use_local_context::<zgui::runtime::windows::Windows>()
        {
            windows.quit();
        }
    }

    /// `:bn`, `:bp`, `:b name`.
    fn buffer(&self, target: BufferTarget) {
        let workspace = &self.inner.workspace;
        match target {
            BufferTarget::Next => workspace.cycle_buffer(1),
            BufferTarget::Previous => workspace.cycle_buffer(-1),
            BufferTarget::First => {
                if let Some(id) = workspace.order().first() {
                    workspace.show(*id);
                }
            }
            BufferTarget::Last => {
                if let Some(id) = workspace.order().last() {
                    workspace.show(*id);
                }
            }
            BufferTarget::Named(named) => {
                // By number first — `:b3` is how vim names one — and then by name.
                if let Ok(number) = named.parse::<usize>()
                    && let Some(id) = workspace.order().get(number.saturating_sub(1))
                {
                    workspace.show(*id);
                    return;
                }
                let found = workspace.order().into_iter().find(|id| {
                    workspace
                        .buffer_untracked(*id)
                        .is_some_and(|buffer| buffer.name().contains(&named))
                });
                match found {
                    Some(id) => workspace.show(id),
                    None => workspace.complain(format!("no buffer matching `{named}`")),
                }
            }
        }
    }

    /// `:%s/old/new/g`.
    fn substitute(
        &self,
        range: &Range,
        pattern: &str,
        replacement: &str,
        all: bool,
        ignore_case: bool,
    ) {
        let workspace = &self.inner.workspace;
        let Some(handle) = workspace.current_handle() else {
            return;
        };
        if pattern.is_empty() {
            return;
        }

        let replacements: Vec<(std::ops::Range<usize>, String)> = handle.query(|snapshot| {
            let rope = snapshot.rope();
            let caret = snapshot.selections().primary().head;
            let on = rope.byte_to_line(caret);
            let lines = range.lines(rope, on, |_| None);

            let mut found = Vec::new();
            for line in lines {
                if line >= rope.len_lines() {
                    break;
                }
                let start = rope.char_to_byte(rope.line_to_char(line));
                let text = rope.line(line).to_string();
                for at in matches_in(&text, pattern, all, ignore_case) {
                    found.push((start + at.start..start + at.end, replacement.to_owned()));
                }
            }
            found
        });

        if replacements.is_empty() {
            workspace.say(format!("no match for `{pattern}`"));
            return;
        }
        let count = replacements.len();
        handle.command(zgui_editor::Command::ReplaceRanges(replacements));
        workspace.say(format!(
            "{count} substitution{}",
            if count == 1 { "" } else { "s" }
        ));
    }

    /// `:set`.
    ///
    /// The names vim uses, mapped onto the settings this editor has. A name it does not know says
    /// so rather than being quietly accepted.
    fn set(&self, name: &str, value: Option<&str>, off: bool, toggle: bool) {
        use zdt_core::config::LineNumbers;

        let workspace = &self.inner.workspace;
        let Some(settings) = zgui::reactive::use_local_context::<crate::settings::Settings>()
        else {
            return;
        };
        // `set name` turns it on, `set noname` off, `set name!` the other way from wherever it is.
        let wanted = |held: bool| if toggle { !held } else { !off };

        let known = match name {
            "number" | "nu" => {
                settings.update(|config| {
                    let on = wanted(config.editor.line_numbers != LineNumbers::None);
                    config.editor.line_numbers = if on {
                        LineNumbers::Absolute
                    } else {
                        LineNumbers::None
                    };
                });
                true
            }
            "relativenumber" | "rnu" => {
                settings.update(|config| {
                    let on = wanted(config.editor.line_numbers == LineNumbers::Relative);
                    config.editor.line_numbers = if on {
                        LineNumbers::Relative
                    } else {
                        LineNumbers::Absolute
                    };
                });
                true
            }
            "cursorline" | "cul" => {
                settings.update(|config| {
                    config.editor.cursorline = wanted(config.editor.cursorline);
                });
                true
            }
            "expandtab" | "et" => {
                settings.update(|config| {
                    config.editor.expand_tab = wanted(config.editor.expand_tab);
                });
                true
            }
            "tabstop" | "ts" | "shiftwidth" | "sw" => match value.and_then(|v| v.parse().ok()) {
                Some(size) => {
                    settings.update(|config| config.editor.tab_size = size);
                    true
                }
                None => false,
            },
            "scrolloff" | "so" => match value.and_then(|v| v.parse().ok()) {
                Some(lines) => {
                    settings.update(|config| config.editor.scrolloff = lines);
                    true
                }
                None => false,
            },
            _ => false,
        };

        if known {
            workspace.say(format!("set {name}"));
        } else {
            workspace.complain(format!("no setting `{name}`"));
        }
    }

    /// A path as typed, against the project when it is relative.
    fn resolve(&self, path: &str) -> std::path::PathBuf {
        let given = std::path::Path::new(path);
        if given.is_absolute() {
            given.to_path_buf()
        } else {
            self.inner.workspace.project().root().join(given)
        }
    }
}

/// Where `pattern` occurs in `text`, as byte ranges.
///
/// Literal, not a regular expression: what people type into `:s` is a word far more often than a
/// pattern, and a half-supported regular expression is worse than an honest literal one.
fn matches_in(
    text: &str,
    pattern: &str,
    all: bool,
    ignore_case: bool,
) -> Vec<std::ops::Range<usize>> {
    let (haystack, needle) = if ignore_case {
        (text.to_lowercase(), pattern.to_lowercase())
    } else {
        (text.to_owned(), pattern.to_owned())
    };
    // Lowercasing can change a string's length, which would put every offset after it wrong.
    let (haystack, needle) = if haystack.len() == text.len() {
        (haystack, needle)
    } else {
        (text.to_owned(), pattern.to_owned())
    };

    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = haystack[from..].find(&needle) {
        let start = from + at;
        found.push(start..start + needle.len());
        if !all {
            break;
        }
        from = start + needle.len().max(1);
        if from > haystack.len() {
            break;
        }
    }
    found
}

/// Puts the command line where every component can find it.
pub fn provide(cmdline: CommandLine) {
    zgui::reactive::provide_local_context(cmdline);
}

/// It, from inside a component.
///
/// # Panics
///
/// If none was provided above this component, which is a wiring mistake.
#[must_use]
pub fn use_cmdline() -> CommandLine {
    zgui::reactive::use_local_context::<CommandLine>().expect("a command line is at the root")
}

#[cfg(test)]
mod tests {
    use super::matches_in;

    #[test]
    fn only_the_first_unless_g_was_asked_for() {
        assert_eq!(matches_in("a a a", "a", false, false), vec![0..1]);
        assert_eq!(
            matches_in("a a a", "a", true, false),
            vec![0..1, 2..3, 4..5]
        );
    }

    #[test]
    fn case_is_ignored_only_when_it_is_asked_for() {
        assert!(matches_in("Alpha", "alpha", true, false).is_empty());
        assert_eq!(matches_in("Alpha", "alpha", true, true), vec![0..5]);
    }

    #[test]
    fn overlapping_text_advances_past_what_it_replaced() {
        // `aa` in `aaaa` is two matches, not three: a substitution replaces what it matched.
        assert_eq!(matches_in("aaaa", "aa", true, false), vec![0..2, 2..4]);
    }

    #[test]
    fn nothing_to_find_is_no_matches() {
        assert!(matches_in("hello", "zzz", true, false).is_empty());
        assert!(matches_in("", "a", true, false).is_empty());
    }
}
