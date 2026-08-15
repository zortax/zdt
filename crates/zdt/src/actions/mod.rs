//! What every named action does.
//!
//! The engine knows `motion.word_forward` and `operator.delete`. Everything else — the pickers,
//! the buffers, the windows, the language servers — reaches here as a name and some arguments,
//! straight out of the keymap file. One `match` is the whole registry.
//!
//! An action nobody has written yet says so in the status line rather than doing nothing, which is
//! what makes a half-built editor say which half.

use std::path::PathBuf;

mod lsp;

use zdt_vim::Action;
use zgui_editor::EditorHandle;

use crate::explorer::Explorer;
use crate::prompt::Prompt;
use crate::settings::Settings;
use crate::vim::Vim;
use crate::workspace::{Axis, Workspace};

/// Carries out `action`.
pub fn run(workspace: &Workspace, vim: &Vim, action: &Action, handle: Option<&EditorHandle>) {
    let leaf = action.leaf();
    let args = &action.args;

    match action.name.split('.').next().unwrap_or("") {
        "buffer" => buffer(workspace, leaf, args),
        "window" => window(workspace, vim, leaf, args),
        "app" => app(workspace, leaf),
        "editor" => editor(handle, leaf),
        "leap" => leap(workspace, vim, leaf),
        "tree" => tree(workspace, leaf, args),
        "picker" => picker(workspace, leaf, args, handle),
        "terminal" => terminal(workspace, vim, leaf, args),
        "lsp" => lsp::run(workspace, leaf, handle),
        "diagnostic" => lsp::diagnostic(workspace, leaf, handle),
        "ui" => ui(workspace, leaf, args),
        // Everything else belongs to a part of the editor that is still being built. Saying so is
        // better than a key that quietly does nothing.
        _ => workspace.say(format!("{} is not built yet", action.name)),
    }
}

/// The buffer commands.
fn buffer(workspace: &Workspace, leaf: &str, args: &zdt_vim::Args) {
    match leaf {
        "save" => {
            if let Some(buffer) = workspace.current_buffer() {
                crate::files::save(workspace, buffer.id);
            }
        }
        "new" => {
            workspace.open_document(None, zgui_editor::Document::new(""));
        }
        "close" => {
            if let Some(buffer) = workspace.current_buffer() {
                if buffer.is_dirty() && !args.flag("force") {
                    workspace.complain("unsaved changes; <Leader>C closes anyway");
                } else {
                    workspace.close_buffer(buffer.id);
                }
            }
        }
        "next" => workspace.cycle_buffer(1),
        "previous" => workspace.cycle_buffer(-1),
        "alternate" => workspace.show_alternate(),
        "move" => workspace.move_buffer(args.number("offset").unwrap_or(1) as isize),
        "close_others" | "close_all" | "close_left" | "close_right" => {
            close_many(workspace, leaf);
        }
        "sort" => workspace.say(format!(
            "sorting by {} is not built yet",
            args.str("by").unwrap_or("")
        )),
        other => workspace.say(format!("buffer.{other} is not built yet")),
    }
}

/// The four ways of closing several buffers at once.
fn close_many(workspace: &Workspace, leaf: &str) {
    let order = workspace.order();
    let Some(current) = workspace.current_buffer().map(|buffer| buffer.id) else {
        return;
    };
    let Some(at) = order.iter().position(|held| *held == current) else {
        return;
    };

    let doomed: Vec<_> = match leaf {
        "close_others" => order
            .iter()
            .filter(|held| **held != current)
            .copied()
            .collect(),
        "close_all" => order.clone(),
        "close_left" => order[..at].to_vec(),
        "close_right" => order[at + 1..].to_vec(),
        _ => Vec::new(),
    };

    // Those with unsaved changes are kept, and said so: closing several at once must not be a way
    // to lose work by accident.
    let mut kept = 0;
    for id in doomed {
        let dirty = workspace
            .buffer_untracked(id)
            .is_some_and(|buffer| buffer.is_dirty());
        if dirty {
            kept += 1;
        } else {
            workspace.close_buffer(id);
        }
    }
    if kept > 0 {
        workspace.complain(format!("{kept} with unsaved changes were kept"));
    }
}

/// The window commands.
fn window(workspace: &Workspace, vim: &Vim, leaf: &str, args: &zdt_vim::Args) {
    match leaf {
        "split" => {
            let axis = match args.str("axis") {
                Some("vertical") => Axis::Horizontal,
                _ => Axis::Vertical,
            };
            workspace.split(axis);
            vim.reset();
        }
        "close" => {
            if !workspace.close_window() {
                workspace.complain("the last window does not close");
            } else {
                vim.reset();
            }
        }
        "cycle" => {
            workspace.cycle_window(true);
            vim.reset();
        }
        // Which window is left, below, above or right of this one needs the geometry of the frame
        // that was drawn, which the panes know and this does not yet.
        "focus" => {
            workspace.cycle_window(!matches!(args.str("direction"), Some("left" | "up")));
            vim.reset();
        }
        other => workspace.say(format!("window.{other} is not built yet")),
    }
}

/// The application itself.
fn app(workspace: &Workspace, leaf: &str) {
    match leaf {
        "quit" => {
            let unsaved = workspace
                .order()
                .into_iter()
                .filter(|id| {
                    workspace
                        .buffer_untracked(*id)
                        .is_some_and(|buffer| buffer.is_dirty())
                })
                .count();
            if unsaved > 0 {
                workspace.complain(format!("{unsaved} buffers have unsaved changes"));
            } else if let Some(windows) =
                zgui::reactive::use_local_context::<zgui::runtime::windows::Windows>()
            {
                windows.quit();
            }
        }
        other => workspace.say(format!("app.{other} is not built yet")),
    }
}

/// The `<Leader>u` toggles.
///
/// Each writes into the settings, which everything that follows one reads — so a toggle is one
/// line here rather than a second copy of the truth beside the configuration.
fn ui(workspace: &Workspace, leaf: &str, args: &zdt_vim::Args) {
    use zdt_core::config::LineNumbers;

    let Some(settings) = zgui::reactive::use_local_context::<Settings>() else {
        return;
    };

    if leaf == "dismiss" {
        workspace.hush();
        return;
    }
    if leaf != "toggle" {
        workspace.say(format!("ui.{leaf} is not built yet"));
        return;
    }

    let setting = args.str("setting").unwrap_or("");
    match setting {
        "scheme" => {
            settings.toggle_scheme();
            let now = settings.with(|config| config.ui.scheme);
            workspace.say(format!("{now:?} theme").to_lowercase());
        }
        "line_numbers" => settings.update(|config| {
            config.editor.line_numbers = match config.editor.line_numbers {
                LineNumbers::None => LineNumbers::Absolute,
                _ => LineNumbers::None,
            };
        }),
        "relative_numbers" => settings.update(|config| {
            config.editor.line_numbers = match config.editor.line_numbers {
                LineNumbers::Relative => LineNumbers::Absolute,
                _ => LineNumbers::Relative,
            };
        }),
        "cursorline" => settings.update(|config| {
            config.editor.cursorline = !config.editor.cursorline;
        }),
        "smooth_scroll" => settings.update(|config| {
            config.editor.smooth_scroll = !config.editor.smooth_scroll;
        }),
        other => workspace.say(format!("there is no `{other}` to toggle yet")),
    }
}

/// The terminals.
///
/// One action, `terminal.toggle`, and the arguments say which one: a float by name, or a window
/// split with a shell in it. The names come from the keymap, so a person who wants a `k9s` float
/// adds a row rather than waiting for one.
fn terminal(workspace: &Workspace, vim: &Vim, leaf: &str, args: &zdt_vim::Args) {
    use crate::terminals::{Program, Terminals};

    let Some(terminals) = zgui::reactive::use_local_context::<Terminals>() else {
        return;
    };

    match leaf {
        "toggle" => {
            let program = match args.str("command") {
                Some(line) => Program::command(line),
                None => Program::shell(),
            };
            match args.str("placement").unwrap_or("float") {
                "float" => {
                    // The name is what makes it the *same* float each time: without one, every
                    // press would start another lazygit.
                    let name = args.str("id").unwrap_or("default");
                    terminals.toggle_float(name, &program);
                }
                placement => {
                    // A split with a terminal in it, which is vim's `:sp | terminal`.
                    let axis = if placement == "vertical" {
                        Axis::Horizontal
                    } else {
                        Axis::Vertical
                    };
                    workspace.split(axis);
                    vim.reset();
                    if let Some(id) = terminals.open(&program) {
                        terminals.start_typing(id);
                    }
                }
            }
        }
        "open" => {
            let program = match args.str("command") {
                Some(line) => Program::command(line),
                None => Program::shell(),
            };
            if let Some(id) = terminals.open(&program) {
                terminals.start_typing(id);
            }
        }
        "normal" => terminals.stop_typing(),
        "hide" => terminals.hide_float(),
        "insert" => {
            if let Some(buffer) = workspace
                .current_buffer()
                .filter(|buffer| buffer.is_terminal())
            {
                terminals.start_typing(buffer.id);
            }
        }
        other => workspace.say(format!("terminal.{other} is not built yet")),
    }
}

/// The pickers.
///
/// One action per source, all of them the same call: the difference between `<Leader>ff` and
/// `<Leader>fb` is which list is gathered, not what the modal does with it.
fn picker(workspace: &Workspace, leaf: &str, args: &zdt_vim::Args, handle: Option<&EditorHandle>) {
    use crate::picker::{Picker, Source};

    let Some(picker) = zgui::reactive::use_local_context::<Picker>() else {
        return;
    };
    let Some(mut source) = Source::named(leaf, args) else {
        workspace.say(format!("picker.{leaf} is not built yet"));
        return;
    };

    // `<Leader>fc` searches for what the caret is on, which is the one thing a picker cannot ask
    // for itself: by the time it is open, the caret is in its own prompt.
    if args.flag("word_under_cursor")
        && let Some(handle) = handle
    {
        let word = handle.query(|snapshot| {
            let caret = snapshot.selections().primary().head;
            let range = snapshot.word_at(caret);
            snapshot.text_in(range)
        });
        if let Source::Grep { start, .. } = &mut source {
            *start = word;
        }
    }

    picker.open(source);
}

/// The file tree.
///
/// Everything that touches the filesystem goes through a worker and reports back, because a
/// directory copy on the interface thread is a frozen window.
fn tree(workspace: &Workspace, leaf: &str, args: &zdt_vim::Args) {
    let Some(explorer) = zgui::reactive::use_local_context::<Explorer>() else {
        return;
    };

    match leaf {
        "toggle" => explorer.toggle(),
        "focus" => explorer.focus(),
        "close" => {
            explorer.close();
            workspace.focus_editor();
        }
        "leave" => {
            explorer.unfocus();
            workspace.focus_editor();
        }
        "down" => explorer.move_by(1),
        "up" => explorer.move_by(-1),
        "first" => explorer.go_to(0),
        "last" => explorer.go_to(usize::MAX),
        "parent_or_close" => explorer.parent_or_close(),
        "child_or_open" => {
            if let Some(path) = explorer.open_selected() {
                crate::files::open(workspace, path);
                // Opening a file gives the keyboard back, the way `<CR>` in neo-tree does.
                explorer.unfocus();
                workspace.focus_editor();
            }
        }
        "refresh" => explorer.refresh(),
        "reveal" => {
            if let Some(path) = workspace.current_buffer().and_then(|buffer| buffer.path) {
                explorer.focus();
                explorer.reveal(&path);
            }
        }
        // Both go through the settings rather than the tree, because the tree follows the
        // settings — writing to the tree directly would be undone the next time anything else
        // changed.
        "toggle_hidden" => {
            let now = with_settings(|config| {
                config.tree.hidden = !config.tree.hidden;
                config.tree.hidden
            });
            if let Some(now) = now {
                workspace.say(if now {
                    "hidden files on"
                } else {
                    "hidden files off"
                });
            }
        }
        "toggle_ignored" => {
            let now = with_settings(|config| {
                config.tree.ignored = !config.tree.ignored;
                config.tree.ignored
            });
            if let Some(now) = now {
                workspace.say(if now {
                    "ignored files on"
                } else {
                    "ignored files off"
                });
            }
        }
        "copy_path" => {
            if let Some(row) = explorer.selected() {
                let path = row.entry.path.display().to_string();
                zgui::runtime::clipboard::use_clipboard()
                    .set_text(zgui::platform::ClipboardKind::Standard, path.clone());
                workspace.say(path);
            }
        }
        "copy" | "cut" => {
            let cut = leaf == "cut";
            if let Some(path) = explorer.hold(cut) {
                workspace.say(format!(
                    "{} {}",
                    if cut { "cut" } else { "copied" },
                    path.file_name().unwrap_or_default().to_string_lossy()
                ));
            }
        }
        "paste" => paste(workspace, &explorer),
        "create" => create(workspace, &explorer, args.flag("directory")),
        "rename" => rename(workspace, &explorer),
        "delete" => delete(workspace, &explorer),
        "system_open" => {
            if let Some(row) = explorer.selected() {
                let path = row.entry.path.clone();
                crate::task::detached(async move {
                    zgui::task::blocking(move || {
                        let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
                    })
                    .await;
                });
            }
        }
        other => workspace.say(format!("tree.{other} is not built yet")),
    }
}

/// Changes a setting and answers what it became, when there are settings to change.
fn with_settings<T>(change: impl FnOnce(&mut zdt_core::Config) -> T) -> Option<T> {
    let settings = zgui::reactive::use_local_context::<Settings>()?;
    let mut answer = None;
    settings.update(|config| answer = Some(change(config)));
    answer
}

/// Asks for a name, then makes one.
fn create(workspace: &Workspace, explorer: &Explorer, directory: bool) {
    let target = explorer.target_directory();
    let title = if directory {
        format!("New directory in {}", short(workspace, &target))
    } else {
        format!("New file in {}", short(workspace, &target))
    };
    ask(workspace, explorer, title, String::new(), move |name| {
        let path = target.join(name.trim_end_matches('/'));
        // A name ending in a separator is a directory, which is how neo-tree's `a` makes one.
        let directory = directory || name.ends_with('/');
        let made = path.clone();
        (
            Box::new(move || zdt_core::paths::create(&path, directory).map(|_| ()))
                as Box<dyn FnOnce() -> std::io::Result<()> + Send>,
            Some(made),
        )
    });
}

/// Asks for a new name, then moves it.
fn rename(workspace: &Workspace, explorer: &Explorer) {
    let Some(row) = explorer.selected() else {
        return;
    };
    let from = row.entry.path.clone();
    let parent = from
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();
    ask(
        workspace,
        explorer,
        format!("Rename {}", row.entry.name),
        row.entry.name.clone(),
        move |name| {
            // Cloned per call: the prompt's answer is typed as callable more than once, even
            // though it only ever is once.
            let from = from.clone();
            let to = parent.join(name);
            let landed = to.clone();
            (
                Box::new(move || zdt_core::paths::rename(&from, &to))
                    as Box<dyn FnOnce() -> std::io::Result<()> + Send>,
                Some(landed),
            )
        },
    );
}

/// Asks whether, then removes it.
fn delete(workspace: &Workspace, explorer: &Explorer) {
    let Some(row) = explorer.selected() else {
        return;
    };
    let path = row.entry.path.clone();
    // Typed confirmation rather than a dialog: the keyboard is already in the tree, and a dialog
    // that takes it away is a dialog people dismiss without reading.
    ask(
        workspace,
        explorer,
        format!("Delete {}? (y/n)", row.entry.name),
        String::new(),
        move |answer| {
            let path = path.clone();
            if !answer.eq_ignore_ascii_case("y") && !answer.eq_ignore_ascii_case("yes") {
                return (
                    Box::new(|| Ok(())) as Box<dyn FnOnce() -> std::io::Result<()> + Send>,
                    None,
                );
            }
            (
                Box::new(move || zdt_core::paths::remove(&path))
                    as Box<dyn FnOnce() -> std::io::Result<()> + Send>,
                None,
            )
        },
    );
}

/// Puts what was held into the selected directory.
fn paste(workspace: &Workspace, explorer: &Explorer) {
    let Some(held) = explorer.clipboard() else {
        workspace.complain("nothing to paste");
        return;
    };
    let target = explorer.target_directory().join(
        held.path
            .file_name()
            .map(std::ffi::OsStr::to_os_string)
            .unwrap_or_default(),
    );
    if target == held.path {
        // Pasting into the directory it already sits in means a copy beside it, not an error.
        if held.cut {
            explorer.release();
            return;
        }
    }

    let from = held.path.clone();
    let cut = held.cut;
    let explorer = explorer.clone();
    let workspace = workspace.clone();
    crate::task::detached(async move {
        let done = zgui::task::blocking(move || {
            let to = zdt_core::paths::free_name(&target);
            if cut {
                zdt_core::paths::rename(&from, &to).map(|()| to)
            } else {
                zdt_core::paths::copy(&from, &to).map(|()| to)
            }
        })
        .await;
        match done {
            Ok(landed) => {
                if cut {
                    explorer.release();
                }
                explorer.refresh();
                workspace.say(format!(
                    "{} {}",
                    if cut { "moved to" } else { "copied to" },
                    landed.display()
                ));
            }
            Err(error) => workspace.complain(error.to_string()),
        }
    });
}

/// The shape every tree prompt has: ask, do the work on a worker, refresh, report.
///
/// `plan` turns the answer into the work and, when there is one, the path the caret should land
/// on afterwards. It runs on the interface thread; only what it returns crosses to the worker.
fn ask<F>(workspace: &Workspace, explorer: &Explorer, title: String, start: String, plan: F)
where
    F: Fn(
            &str,
        ) -> (
            Box<dyn FnOnce() -> std::io::Result<()> + Send>,
            Option<PathBuf>,
        ) + 'static,
{
    let Some(prompt) = zgui::reactive::use_local_context::<Prompt>() else {
        return;
    };
    let explorer = explorer.clone();
    let workspace = workspace.clone();
    prompt.ask(title, start, move |answer| {
        let (work, landing) = plan(answer);
        let explorer = explorer.clone();
        let workspace = workspace.clone();
        // Detached, because submitting the prompt is what closed it: a task belonging to the
        // prompt would be cancelled before the file was ever made.
        crate::task::detached(async move {
            match zgui::task::blocking(work).await {
                Ok(()) => {
                    explorer.refresh();
                    if let Some(landing) = landing {
                        explorer.reveal(&landing);
                    }
                }
                Err(error) => workspace.complain(error.to_string()),
            }
            explorer.focus();
        });
    });
}

/// A path as it reads in a message: relative to the project when it is under it.
///
/// The project root itself is relative to nothing, so it reads as its own name rather than as the
/// empty string that "relative to here" literally comes to.
fn short(workspace: &Workspace, path: &std::path::Path) -> String {
    let relative = workspace.project().relative(path).into_owned();
    if relative.is_empty() {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
    } else {
        relative
    }
}

/// Leap.
///
/// One action; the argument says which way it looks. Everything after the key that started it
/// belongs to the leap layer rather than to the keymap.
fn leap(workspace: &Workspace, vim: &Vim, leaf: &str) {
    use zdt_vim::leap::Direction;

    match leaf {
        "forward" => vim.start_leap(Direction::Forward),
        "backward" => vim.start_leap(Direction::Backward),
        // `gs` leaps into another window in leap.nvim. There is one window's worth of labels here
        // until the panes can say where each other's text is on screen, so it leaps both ways in
        // this one — which is the useful half of it, and says so rather than doing nothing.
        "window" => vim.start_leap(Direction::Both),
        other => workspace.say(format!("leap.{other} is not built yet")),
    }
}

/// The few things that are the editor's own.
fn editor(handle: Option<&EditorHandle>, leaf: &str) {
    if leaf == "focus"
        && let Some(handle) = handle
    {
        handle.focus();
    }
}
