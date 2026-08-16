//! What the interface reads.

use super::*;

impl Language {
    // ---- What the interface reads ------------------------------------------------------------

    /// A number that changes whenever anything a view draws has. Tracked.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.inner.revision.get()
    }

    /// What the servers are busy with, when they say. Tracked.
    #[must_use]
    pub fn busy(&self) -> Option<String> {
        self.inner.busy.get()
    }

    /// What the servers answering for `path` are doing. Tracked.
    ///
    /// The worst thing any of them is doing, in the order a person would want to hear it: a
    /// failure first, then work in progress, then readiness. A file two servers answer for is one
    /// line in the status line, and the line has to say the thing worth acting on.
    #[must_use]
    pub fn state(&self, path: Option<&Path>) -> ServerState {
        // Read first, so this follows the pool.
        let _ = self.inner.revision.get();
        let busy = self.inner.busy.get().is_some();

        if !self.inner.enabled.get() {
            return ServerState::Inactive;
        }
        let Some(path) = path else {
            return ServerState::Inactive;
        };
        let files = self.inner.files.borrow();
        let Some(keys) = files.get(path) else {
            return ServerState::Inactive;
        };

        let pool = self.inner.pool.borrow();
        let mut state = ServerState::Inactive;
        for key in keys {
            let found = match pool.state(key) {
                Some(zdt_lsp::pool::Asked::Running) if busy => ServerState::Indexing,
                Some(zdt_lsp::pool::Asked::Running) => ServerState::Ready,
                Some(zdt_lsp::pool::Asked::Starting) => ServerState::Starting,
                Some(zdt_lsp::pool::Asked::Failed(_)) => ServerState::Failed,
                None => ServerState::Inactive,
            };
            if rank(found) > rank(state) {
                state = found;
            }
        }
        state
    }

    /// Everything wrong with `path`.
    #[must_use]
    pub fn diagnostics(&self, path: &Path) -> Vec<lsp_types::Diagnostic> {
        self.inner.store.borrow().for_file(path)
    }

    /// Every file anything has been said about.
    ///
    /// Which is not every file in the project: a server publishes about what it has looked at, and
    /// what it has looked at is what somebody has opened plus whatever the project pulled in.
    #[must_use]
    pub fn files(&self) -> Vec<PathBuf> {
        self.inner.store.borrow().files()
    }

    /// How many of each kind `path` has.
    #[must_use]
    pub fn counts(&self, path: &Path) -> Counts {
        self.inner.store.borrow().counts(path)
    }

    /// The next diagnostic after `line`, wrapping.
    #[must_use]
    pub fn after(&self, path: &Path, line: u32) -> Option<lsp_types::Diagnostic> {
        self.inner.store.borrow().after(path, line)
    }

    /// The one before it, wrapping.
    #[must_use]
    pub fn before(&self, path: &Path, line: u32) -> Option<lsp_types::Diagnostic> {
        self.inner.store.borrow().before(path, line)
    }

    /// Everything on `line`.
    #[must_use]
    pub fn on_line(&self, path: &Path, line: u32) -> Vec<lsp_types::Diagnostic> {
        self.inner.store.borrow().on_line(path, line)
    }

    /// The file the editor is showing, when it is showing a file.
    ///
    /// Here and not at the call sites, because every language request wants it and the walk
    /// from workspace to buffer to path is three lines each time.
    #[must_use]
    pub fn current_path(&self) -> Option<PathBuf> {
        self.inner
            .workspace
            .current_buffer()
            .and_then(|buffer| buffer.path)
    }

    /// The file `handle` is editing, when it is one this layer knows about.
    ///
    /// Found by asking every window. A request started in one window can be answered while the
    /// keyboard is in another.
    #[must_use]
    pub fn path_of_handle(&self, handle: &zgui_editor::EditorHandle) -> Option<PathBuf> {
        let workspace = &self.inner.workspace;
        for buffer in workspace.order() {
            for window in workspace.windows() {
                if workspace
                    .handle_for(window, buffer)
                    .is_some_and(|held| held == *handle)
                {
                    return workspace.buffer_untracked(buffer).and_then(|one| one.path);
                }
            }
        }
        self.current_path()
    }

    /// Which servers are answering for `path`.
    #[must_use]
    pub fn servers_for(&self, path: &Path) -> Vec<String> {
        self.inner
            .files
            .borrow()
            .get(path)
            .map(|keys| keys.iter().map(|(name, _)| name.clone()).collect())
            .unwrap_or_default()
    }
}
