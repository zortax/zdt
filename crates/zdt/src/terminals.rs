//! The terminals.
//!
//! Two kinds, and they are the same thing arranged differently.
//!
//! A **buffer terminal** is a `BufferKind::Terminal` on the buffer line: `]b` walks onto it,
//! `<Leader>c` closes it, and it is a window's contents like any file. That is vim's `:terminal`.
//!
//! A **floating terminal** is one of a handful kept by name — the default one, lazygit, python —
//! shown over everything and toggled with a key. That is toggleterm's, and the reason it is not
//! just a buffer is that it must be reachable from anywhere without disturbing what is on screen.
//!
//! # Why the transports are held here
//!
//! The component takes its transport when it is built and never again, but a buffer exists before
//! any window has drawn it and outlives every view that has. So the process is started here, at
//! the moment somebody asks for a terminal, and left in [`Terminals`] until a view mounts and
//! takes it. A terminal buffer that is never drawn holds a program that is never read from, which
//! is why closing one shuts the program down rather than leaking it.

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
    /// Programs that have been started and not yet drawn, waiting for a view to take them.
    pending: RefCell<FxHashMap<BufferId, Pty>>,
    /// The handle of each terminal that is on screen.
    handles: RefCell<FxHashMap<BufferId, TerminalHandle>>,
    /// The floating terminals, by the name they are asked for.
    floats: RefCell<FxHashMap<String, BufferId>>,
    /// Which float is being shown, when one is.
    showing: RwSignal<Option<BufferId>, LocalStorage>,
    /// Which terminal has the keyboard, and is therefore taking keys away from the keymap.
    typing: RwSignal<Option<BufferId>, LocalStorage>,
    /// Whether `<C-\>` has been pressed and the `<C-n>` that would complete it is awaited.
    ///
    /// Held here rather than in the vim engine, because the engine is not answering while a
    /// terminal is: the whole point of terminal mode is that the keys go elsewhere.
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
                pending: RefCell::new(FxHashMap::default()),
                handles: RefCell::new(FxHashMap::default()),
                floats: RefCell::new(FxHashMap::default()),
                showing: RwSignal::new_local(None),
                typing: RwSignal::new_local(None),
                escaping: std::cell::Cell::new(false),
            }),
        }
    }

    // ---- Making them --------------------------------------------------------------------------

    /// Starts `program` in a buffer on the buffer line, and shows it.
    ///
    /// Answers the buffer, or nothing when the program could not be started — which is said in the
    /// status line rather than returned, because every caller would only say the same thing.
    pub fn open(&self, program: &Program) -> Option<BufferId> {
        self.spawn(program, true)
    }

    /// Starts `program` in a buffer.
    ///
    /// `listed` puts it on the buffer line and shows it. A float is not listed: it is drawn over
    /// everything by the float itself, and a window that also drew it would be a second view onto
    /// one program — which the emulator cannot be, because the transport is taken once.
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

        let id = self.inner.workspace.open_terminal(&program.name(), listed);
        self.inner.pending.borrow_mut().insert(id, pty);
        Some(id)
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

    /// Takes the program waiting for `buffer`, if one is.
    ///
    /// Called once, by the view that is about to draw it. A second call answers nothing, which is
    /// what stops a remount from starting a second shell.
    pub fn take_pending(&self, buffer: BufferId) -> Option<Pty> {
        self.inner.pending.borrow_mut().remove(&buffer)
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
    pub fn close(&self, buffer: BufferId) {
        if let Some(handle) = self.inner.handles.borrow_mut().remove(&buffer) {
            handle.shutdown();
        }
        self.inner.pending.borrow_mut().remove(&buffer);
        self.inner
            .floats
            .borrow_mut()
            .retain(|_, held| *held != buffer);
        if self.inner.showing.get_untracked() == Some(buffer) {
            self.inner.showing.set(None);
        }
        if self.inner.typing.get_untracked() == Some(buffer) {
            self.inner.typing.set(None);
        }
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
                self.inner.typing.set(Some(id));
            }
            return;
        }

        let Some(id) = self.spawn(program, false) else {
            return;
        };
        self.inner.floats.borrow_mut().insert(name.to_owned(), id);
        self.inner.showing.set(Some(id));
        self.inner.typing.set(Some(id));
    }

    /// Puts the float away, leaving the program running.
    pub fn hide_float(&self) {
        if self.inner.showing.get_untracked().is_some() {
            self.inner.showing.set(None);
        }
        if self.inner.typing.get_untracked().is_some() {
            self.inner.typing.set(None);
            self.inner.workspace.focus_editor();
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

    // ---- Who has the keyboard ---------------------------------------------------------------

    /// Which terminal the keys are going to. Tracked.
    #[must_use]
    pub fn typing(&self) -> Option<BufferId> {
        self.inner.typing.get()
    }

    /// The same, without subscribing.
    #[must_use]
    pub fn typing_untracked(&self) -> Option<BufferId> {
        self.inner.typing.get_untracked()
    }

    /// Gives the keys to the terminal in `buffer` — vim's terminal mode.
    pub fn start_typing(&self, buffer: BufferId) {
        if self.inner.typing.get_untracked() != Some(buffer) {
            self.inner.typing.set(Some(buffer));
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

    /// Takes them back, which is what `<C-\><C-n>` does.
    ///
    /// The terminal stays where it is and the program keeps running; what changes is that the
    /// keymap answers again, so the scrollback can be walked with vim's own motions.
    pub fn stop_typing(&self) {
        if self.inner.typing.get_untracked().is_some() {
            self.inner.typing.set(None);
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
