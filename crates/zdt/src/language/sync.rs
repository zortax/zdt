//! Keeping the servers in step with the buffers.

use super::*;

impl Language {
    // ---- Keeping up with the buffers ----------------------------------------------------------

    /// Says a buffer has been opened, starting whatever servers claim it.
    pub fn opened(&self, buffer: BufferId) {
        if !self.inner.enabled.get() {
            return;
        }
        let Some((path, language, text)) = self.about(buffer) else {
            return;
        };

        let servers = self.wanted(&language, &path);
        if servers.is_empty() {
            return;
        }

        let mut keys = Vec::new();
        for server in servers {
            let key = Pool::key_of(&server);
            keys.push(key.clone());

            let mut pool = self.inner.pool.borrow_mut();
            if pool.get_mut(&key).is_some() {
                // Already running: tell it about this file now.
                let version = self.next_version(&path);
                if let Some(client) = pool.get_mut(&key) {
                    client.open(&path, &language, version, text.clone());
                }
                continue;
            }
            if !pool.begin(&server, &path) {
                continue;
            }
            drop(pool);
            self.start(server);
        }

        self.inner.files.borrow_mut().insert(path, keys);
    }

    /// Says a buffer's text has changed, after a pause.
    ///
    /// Whole-text, and not incremental. The editor reports what changed, and a rope re-read here
    /// is the one thing certain to be what the buffer holds. Sending a file costs little beside
    /// being subtly out of step with a server. Incremental sync is worth doing once a test can
    /// prove it stays in step.
    pub fn changed(&self, buffer: BufferId) {
        if !self.inner.enabled.get() {
            return;
        }
        let Some((path, _, _)) = self.about(buffer) else {
            return;
        };
        let language = self.clone();
        let waiting = path.clone();
        let handle = self.inner.clock.after(SYNC_DEBOUNCE, move || {
            language.inner.pending.borrow_mut().remove(&waiting);
            language.send_change(buffer);
        });
        // Replacing the handle cancels the one before it, which is the debounce.
        self.inner.pending.borrow_mut().insert(path, handle);
    }

    /// Sends whatever change is still waiting, now.
    ///
    /// Completion asks about the text as it is. A request that raced the debounce would be
    /// answered against the text as it was, and the answer's edit ranges would land beside the
    /// word rather than on it.
    pub fn flush_changes(&self, buffer: BufferId) {
        if !self.inner.enabled.get() {
            return;
        }
        let Some((path, _, _)) = self.about(buffer) else {
            return;
        };
        // Only when something waits: the debounce handle is the sign of an unsent change, and
        // dropping it cancels the send this replaces.
        if self.inner.pending.borrow_mut().remove(&path).is_none() {
            return;
        }
        self.send_change(buffer);
    }

    /// Says a buffer has been written.
    pub fn saved(&self, buffer: BufferId) {
        if !self.inner.enabled.get() {
            return;
        }
        // Anything still waiting to be sent goes first: a server told about a save before the
        // change that preceded it would lint the version before last.
        self.send_change(buffer);

        let Some((path, _, _)) = self.about(buffer) else {
            return;
        };
        self.with_clients(&path, |client, path| client.save(path, None));
    }

    /// Says a buffer has been closed.
    pub fn closed(&self, path: &Path) {
        self.with_clients(path, |client, path| client.close(path));
        self.inner.files.borrow_mut().remove(path);
        self.inner.versions.borrow_mut().remove(path);
        self.inner.pending.borrow_mut().remove(path);
        self.inner.store.borrow_mut().forget(path);
        self.touch();
    }

    /// Reads the settings again: whether servers are wanted, and which.
    ///
    /// A server that could not be started before is allowed to be tried again, because the reason
    /// it failed may be exactly what was just changed.
    pub fn reconfigure(&self) {
        let enabled = self
            .inner
            .settings
            .with_untracked(|config| config.lsp.enabled);
        self.inner.enabled.set(enabled);
        self.inner.pool.borrow_mut().clear_failures();
        if !enabled {
            self.stop_all();
        }
    }

    /// Shuts every server down.
    pub fn stop_all(&self) {
        let clients = self.inner.pool.borrow_mut().drain();
        self.inner.files.borrow_mut().clear();
        *self.inner.store.borrow_mut() = Store::new();
        if self.inner.busy.get_untracked().is_some() {
            self.inner.busy.set(None);
        }
        self.touch();

        for mut client in clients {
            // A server stopped on purpose leaves nothing behind: the row saying it was starting,
            // or that it is ready, is about a server that is now gone.
            self.forget_announcement(&client.name);
            zdt_view::detached(async move {
                if let Err(error) = client.shutdown().await {
                    tracing::debug!("{}: {error}", client.name);
                }
            });
        }
    }

    // ---- Asking ------------------------------------------------------------------------------

    /// The client answering for `path`, cloned so it can be taken to a worker.
    ///
    /// The first one, when several answer. A request has one answer, and somebody pressing `gd`
    /// means the definition.
    #[must_use]
    pub fn client_for(&self, path: &Path) -> Option<zdt_lsp::Client> {
        let files = self.inner.files.borrow();
        let keys = files.get(path)?;
        let pool = self.inner.pool.borrow();
        keys.iter().find_map(|key| pool.get(key).cloned())
    }
}
