//! Carrying the commands out.

use super::*;

impl CommandLine {
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
                // By number first, because `:b3` is how vim names one. Then by name.
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
    /// so.
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
