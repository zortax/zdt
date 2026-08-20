//! The terminals.
//!
//! Two kinds, and they are the same thing arranged differently.
//!
//! A **buffer terminal** is a `BufferKind::Terminal` on the buffer line: `]b` walks onto it,
//! `<Leader>c` closes it, and it is a window's contents like any file. That is vim's `:terminal`.
//!
//! A **floating terminal** is one of a handful kept by name, such as the default one, lazygit or
//! python. It is shown over everything and toggled with a key. That is toggleterm's, and it is
//! more than a buffer because it must be reachable from anywhere without disturbing what is on
//! screen.
//!
//! # Why the transports are held here
//!
//! The component takes its transport when it is built and never again. A buffer exists before any
//! window has drawn it, and outlives every view that has. So the process is started here, at the
//! moment somebody asks for a terminal, and left in [`Terminals`] until a view mounts and takes
//! it. A terminal buffer that is never drawn holds a program that is never read from, so closing
//! one shuts the program down.

pub mod view;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use portable_pty::CommandBuilder;
use rustc_hash::FxHashMap;
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui_terminal::TerminalHandle;
use zgui_terminal::transport::Pty;

use crate::workspace::{BufferId, Workspace};

/// Every terminal there is.
#[derive(Clone)]
pub struct Terminals {
    inner: Rc<Inner>,
}

struct Inner {
    workspace: Workspace,
    settings: crate::settings::Settings,
    /// Every running program, whether or not anything is drawing it.
    ///
    /// The emulator belongs here rather than to a view, so a session taken off screen — hidden,
    /// evicted, or in a window somebody closed — keeps its shells running. See
    /// [`zgui_terminal::TerminalSession`].
    sessions: RefCell<FxHashMap<BufferId, zgui_terminal::TerminalSession>>,
    /// The handle of each terminal that is on screen.
    handles: RefCell<FxHashMap<BufferId, TerminalHandle>>,
    /// The floating terminals, by the name they are asked for.
    floats: RefCell<FxHashMap<String, BufferId>>,
    /// What each terminal was started as, so a session can start it again.
    programs: RefCell<FxHashMap<BufferId, Program>>,
    /// Which float is being shown, when one is.
    showing: RwSignal<Option<BufferId>, LocalStorage>,
    /// Every terminal that is taking keys away from the keymap.
    ///
    /// A set, and never one answer for the session: being in insert is a fact about a terminal, the
    /// way it is in vim. Walking out of a split and back finds the terminal as it was left, and a
    /// terminal nobody is looking at names no mode at all. Which of these has the keyboard is
    /// [`crate::focus`]'s question.
    inserting: RwSignal<rustc_hash::FxHashSet<BufferId>, LocalStorage>,
    /// The window a terminal was given a split of its own for, when it was.
    ///
    /// A terminal opened with `<Leader>tv` gets a window made for it, and that window has nothing
    /// to show once the terminal has gone, so it goes too. One opened into a window that was
    /// already showing something stays out of here, and closing it goes back to what was there.
    owned_windows: RefCell<FxHashMap<BufferId, crate::workspace::WindowId>>,
    /// Whether `<C-\>` has been pressed and the `<C-n>` that would complete it is awaited.
    ///
    /// Held here, and not in the vim engine. The engine is silent while a terminal is answering,
    /// because the whole point of terminal mode is that the keys go elsewhere.
    escaping: std::cell::Cell<bool>,
}

/// What to run, and where.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Program {
    /// The program and its arguments. Empty means the login shell.
    pub argv: Vec<String>,
    /// Where it starts. The project root when not given.
    pub directory: Option<PathBuf>,
}

impl Program {
    /// The shell, in the project root.
    #[must_use]
    pub fn shell() -> Self {
        Self {
            argv: Vec::new(),
            directory: None,
        }
    }

    /// One command, split on spaces the way a keymap writes it.
    #[must_use]
    pub fn command(line: &str) -> Self {
        Self {
            argv: line.split_whitespace().map(str::to_owned).collect(),
            directory: None,
        }
    }

    /// What the buffer line calls it.
    #[must_use]
    pub fn name(&self) -> String {
        self.argv
            .first()
            .map(|program| {
                std::path::Path::new(program)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| program.clone())
            })
            .unwrap_or_else(|| "shell".to_owned())
    }
}

impl Terminals {
    /// No terminals yet.
    #[must_use]
    pub fn new(workspace: Workspace, settings: crate::settings::Settings) -> Self {
        Self {
            inner: Rc::new(Inner {
                workspace,
                settings,
                sessions: RefCell::new(FxHashMap::default()),
                handles: RefCell::new(FxHashMap::default()),
                floats: RefCell::new(FxHashMap::default()),
                programs: RefCell::new(FxHashMap::default()),
                showing: RwSignal::new_local(None),
                inserting: RwSignal::new_local(rustc_hash::FxHashSet::default()),
                owned_windows: RefCell::new(FxHashMap::default()),
                escaping: std::cell::Cell::new(false),
            }),
        }
    }

    // ---- Making them --------------------------------------------------------------------------

    /// Starts `program` in a buffer on the buffer line, and shows it.
    ///
    /// Answers the buffer, or nothing when the program could not be started. The reason goes to
    /// the status line, because every caller would only say the same thing.
    pub fn open(&self, program: &Program) -> Option<BufferId> {
        self.spawn(program, true)
    }

    /// Starts `program` in a buffer.
    ///
    /// `listed` puts it on the buffer line and shows it. A float stays unlisted. The float draws
    /// it over everything, and a window that also drew it would be a second view onto one program.
    /// The emulator cannot be that, because the transport is taken once.
    pub fn spawn(&self, program: &Program, listed: bool) -> Option<BufferId> {
        let pty = match Pty::spawn(self.command(program)) {
            Ok(pty) => pty,
            Err(error) => {
                self.inner
                    .workspace
                    .complain(format!("cannot start {}: {error}", program.name()));
                return None;
            }
        };
        // A size the first frame corrects. The program is told the real one as soon as anything
        // measures a cell, and until then eighty by twenty-four is what every terminal assumes.
        let started = zgui_terminal::TerminalSession::start(
            Box::new(pty),
            self.config(),
            zgui_terminal::transport::TerminalSize {
                columns: 80,
                lines: 24,
                cell_width: 8,
                cell_height: 16,
            },
        );
        let running = match started {
            Ok(running) => running,
            Err(error) => {
                self.inner
                    .workspace
                    .complain(format!("cannot start {}: {error}", program.name()));
                return None;
            }
        };

        let id = self.inner.workspace.open_terminal(&program.name(), listed);
        self.inner.sessions.borrow_mut().insert(id, running);
        // Remembered so a session can start the same thing again. The contents cannot come back,
        // but what was being run and where can.
        self.inner.programs.borrow_mut().insert(id, program.clone());
        Some(id)
    }

    /// What `buffer` was started as, and which float it is, for a session to write down.
    #[must_use]
    pub fn spec_for(&self, buffer: BufferId) -> Option<(Program, Option<String>)> {
        let program = self.inner.programs.borrow().get(&buffer).cloned()?;
        let float = self
            .inner
            .floats
            .borrow()
            .iter()
            .find(|(_, held)| **held == buffer)
            .map(|(name, _)| name.clone());
        Some((program, float))
    }

    /// Starts `program` again for a session being restored, without showing it.
    ///
    /// The buffer it makes takes the place the session had it in. A float is registered under the
    /// name it had, so the key that toggles it finds it again.
    pub fn restore(
        &self,
        program: &Program,
        listed: bool,
        float: Option<&str>,
    ) -> Option<BufferId> {
        let id = self.spawn(program, listed)?;
        if let Some(name) = float {
            self.inner.floats.borrow_mut().insert(name.to_owned(), id);
        }
        Some(id)
    }

    /// How a terminal is drawn, as the settings say.
    fn config(&self) -> zgui_terminal::TerminalConfig {
        crate::terminals::view::emulator::terminal_config(Some(&self.inner.settings))
    }

    /// The command a program comes to, with the environment a terminal is expected to have.
    fn command(&self, program: &Program) -> CommandBuilder {
        let mut command = match program.argv.split_first() {
            Some((first, rest)) => {
                let mut command = CommandBuilder::new(first);
                command.args(rest);
                command
            }
            None => CommandBuilder::new(self.shell()),
        };

        // What a program looks up to learn what this terminal can do. The ordinary name, because
        // it has to be one that is installed at the far end of a connection too.
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        let directory = program
            .directory
            .clone()
            .unwrap_or_else(|| self.inner.workspace.project().root().to_path_buf());
        command.cwd(directory);
        command
    }

    /// Which shell to start, as the settings say or as the environment does.
    fn shell(&self) -> String {
        let configured = self
            .inner
            .settings
            .with_untracked(|config| config.terminal.shell.clone());
        if !configured.is_empty() {
            return configured;
        }
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned())
    }

    /// The program running behind `buffer`, if one is.
    ///
    /// Borrowed rather than taken: a view mounting again draws the same program, which is what
    /// makes a terminal survive being taken off screen and put back.
    #[must_use]
    pub fn running(&self, buffer: BufferId) -> Option<zgui_terminal::TerminalSession> {
        self.inner.sessions.borrow().get(&buffer).cloned()
    }

    // ---- Driving them -------------------------------------------------------------------------

    /// Remembers the handle a view has just built.
    pub fn register(&self, buffer: BufferId, handle: TerminalHandle) {
        self.inner.handles.borrow_mut().insert(buffer, handle);
    }

    /// Forgets it, which a view does as it unmounts.
    pub fn forget(&self, buffer: BufferId) {
        self.inner.handles.borrow_mut().remove(&buffer);
    }

    /// The terminal in `buffer`, when one is on screen.
    #[must_use]
    pub fn handle(&self, buffer: BufferId) -> Option<TerminalHandle> {
        self.inner.handles.borrow().get(&buffer).cloned()
    }

    /// Shuts the program in `buffer` down and forgets everything about it.
    /// Remembers that `window` was split off for `buffer` and should go when it does.
    pub fn owns_window(&self, buffer: BufferId, window: crate::workspace::WindowId) {
        self.inner.owned_windows.borrow_mut().insert(buffer, window);
    }

    /// Ends a terminal and takes away what it was in.
    ///
    /// The program is shut down, the buffer closed, and a window that was split off for it closed
    /// with it: a split made for a terminal has nothing left to show, and leaving it behind
    /// duplicates whatever is beside it.
    pub fn end(&self, workspace: &Workspace, buffer: BufferId) {
        let window = self.inner.owned_windows.borrow_mut().remove(&buffer);
        self.close(buffer);
        workspace.close_buffer(buffer);
        if let Some(window) = window {
            workspace.close_window_at(window);
        }
    }

    pub fn close(&self, buffer: BufferId) {
        if let Some(handle) = self.inner.handles.borrow_mut().remove(&buffer) {
            handle.shutdown();
        }
        // The last handle to the program goes, which is what stops it.
        self.inner.sessions.borrow_mut().remove(&buffer);
        self.inner.owned_windows.borrow_mut().remove(&buffer);
        self.inner
            .floats
            .borrow_mut()
            .retain(|_, held| *held != buffer);
        if self.inner.showing.get_untracked() == Some(buffer) {
            self.inner.showing.set(None);
        }
        self.stop_typing(buffer);
    }

    // ---- The floating ones ----------------------------------------------------------------------

    /// Shows the float called `name`, starting it the first time.
    ///
    /// Toggling: asking for the one that is already showing puts it away.
    pub fn toggle_float(&self, name: &str, program: &Program) {
        let held = self.inner.floats.borrow().get(name).copied();

        if let Some(id) = held {
            if self.inner.showing.get_untracked() == Some(id) {
                self.hide_float();
            } else {
                self.inner.showing.set(Some(id));
                self.start_typing(id);
            }
            return;
        }

        let Some(id) = self.spawn(program, false) else {
            return;
        };
        self.inner.floats.borrow_mut().insert(name.to_owned(), id);
        self.inner.showing.set(Some(id));
        self.start_typing(id);
    }

    /// Puts the float away, leaving the program running.
    ///
    /// Nothing here hands the keyboard back. The float is an overlay, so it holds the keys while it
    /// is shown and the region underneath gets them when it goes. See [`crate::focus`].
    pub fn hide_float(&self) {
        if self.inner.showing.get_untracked().is_some() {
            self.inner.showing.set(None);
        }
    }

    /// Which float is showing. Tracked.
    #[must_use]
    pub fn showing(&self) -> Option<BufferId> {
        self.inner.showing.get()
    }

    /// Every float there is, by name, whether or not it is showing.
    #[must_use]
    pub fn floats(&self) -> Vec<(String, BufferId)> {
        let mut found: Vec<(String, BufferId)> = self
            .inner
            .floats
            .borrow()
            .iter()
            .map(|(name, id)| (name.clone(), *id))
            .collect();
        found.sort_by(|left, right| left.0.cmp(&right.0));
        found
    }

    // ---- Which terminals are taking keys -------------------------------------------------------

    /// Whether the program in `buffer` is being typed into. Tracked.
    #[must_use]
    pub fn is_inserting(&self, buffer: BufferId) -> bool {
        self.inner.inserting.with(|held| held.contains(&buffer))
    }

    /// The same, without subscribing.
    #[must_use]
    pub fn is_inserting_untracked(&self, buffer: BufferId) -> bool {
        self.inner
            .inserting
            .with_untracked(|held| held.contains(&buffer))
    }

    /// Gives the keys to the terminal in `buffer`. This is vim's terminal mode.
    pub fn start_typing(&self, buffer: BufferId) {
        if !self.is_inserting_untracked(buffer) {
            self.inner.inserting.update(|held| {
                held.insert(buffer);
            });
        }
    }

    /// Says that `<C-\>` has been seen and the next key may complete the way out.
    pub fn expect_normal(&self) {
        self.inner.escaping.set(true);
    }

    /// Whether it has.
    #[must_use]
    pub fn expecting_normal(&self) -> bool {
        self.inner.escaping.get()
    }

    /// Forgets it, which the next key does whether or not it completed anything.
    pub fn clear_expectation(&self) {
        self.inner.escaping.set(false);
    }

    /// Takes them back from the terminal in `buffer`, which is what `<C-\><C-n>` does.
    ///
    /// The terminal stays where it is and the program keeps running; what changes is that the
    /// keymap answers again, so the scrollback can be walked with vim's own motions. Remembered
    /// per terminal, so coming back to this one finds it as it was left.
    pub fn stop_typing(&self, buffer: BufferId) {
        if self.is_inserting_untracked(buffer) {
            self.inner.inserting.update(|held| {
                held.remove(&buffer);
            });
        }
    }
}

/// Puts the terminals where every component can find them.
pub fn provide(terminals: Terminals) {
    zgui::reactive::provide_local_context(terminals);
}

/// The terminals, from inside a component.
///
/// # Panics
///
/// If none were provided above this component, which is a wiring mistake.
#[must_use]
pub fn use_terminals() -> Terminals {
    zgui::reactive::use_local_context::<Terminals>().expect("terminals are provided at the root")
}
