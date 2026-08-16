//! What a server says without being asked.

use super::*;

impl Language {
    // ---- Internals ---------------------------------------------------------------------------

    /// Starts a server, and tells it about everything that was waiting for it.
    pub(super) fn start(&self, wanted: Wanted) {
        let language = self.clone();
        let notices = self.inner.notices.clone();
        self.announce(
            &wanted.name,
            Toast::new(format!("starting {}", wanted.name))
                .kind(ToastKind::Loading)
                .persistent(),
        );
        zdt_view::detached(async move {
            let started = {
                let wanted = wanted.clone();
                zgui::task::background(async move { zdt_lsp::pool::start(&wanted, notices).await })
                    .await
            };

            match started {
                Ok(client) => {
                    let waiting = language.inner.pool.borrow_mut().arrived(client);
                    for path in waiting {
                        language.open_now(&wanted, &path);
                    }
                    // The row the "starting" announcement was holding becomes this one, rather
                    // than a second row saying the opposite of the first.
                    language.announce(
                        &wanted.name,
                        Toast::new(format!("{} is ready", wanted.name)).kind(ToastKind::Success),
                    );
                }
                Err(error) => {
                    language.inner.pool.borrow_mut().failed(&wanted, &error);
                    language.announce(
                        &wanted.name,
                        Toast::new(format!("{} did not start", wanted.name))
                            .kind(ToastKind::Error)
                            .description(error.to_string())
                            .persistent(),
                    );
                }
            }
            language.touch();
        });
    }

    /// Tells a just-started server about a file that was open before it was.
    fn open_now(&self, wanted: &Wanted, path: &Path) {
        let Some(buffer) = self.inner.workspace.find_path(path) else {
            return;
        };
        let Some((path, language, text)) = self.about(buffer) else {
            return;
        };
        let version = self.next_version(&path);
        let key = Pool::key_of(wanted);
        if let Some(client) = self.inner.pool.borrow_mut().get_mut(&key) {
            client.open(&path, &language, version, text);
        }
    }

    /// Tells every server answering for a buffer what it now holds.
    pub(super) fn send_change(&self, buffer: BufferId) {
        let Some((path, _, text)) = self.about(buffer) else {
            return;
        };
        let version = self.next_version(&path);
        self.with_clients(&path, |client, path| {
            client.change(
                path,
                version,
                vec![lsp_types::TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: text.clone(),
                }],
            );
        });
    }

    /// Runs `act` against every client answering for `path`.
    pub(super) fn with_clients(
        &self,
        path: &Path,
        mut act: impl FnMut(&mut zdt_lsp::Client, &Path),
    ) {
        let keys = self.inner.files.borrow().get(path).cloned();
        let Some(keys) = keys else {
            return;
        };
        let mut pool = self.inner.pool.borrow_mut();
        for key in keys {
            if let Some(client) = pool.get_mut(&key) {
                act(client, path);
            }
        }
    }

    /// The path, language and text of a buffer, when it has all three.
    pub(super) fn about(&self, buffer: BufferId) -> Option<(PathBuf, String, String)> {
        let entry = self.inner.workspace.buffer_untracked(buffer)?;
        let path = entry.path.clone()?;
        let language = entry.language()?.to_owned();
        let text = entry.document()?.text();
        Some((path, language, text))
    }

    /// Which servers claim a file.
    pub(super) fn wanted(&self, language: &str, path: &Path) -> Vec<Wanted> {
        let root = self.inner.workspace.project().root().to_path_buf();
        self.inner.settings.with_untracked(|config| {
            zdt_lsp::registry::wanted_for(&config.lsp.servers, language, path, &root)
        })
    }

    /// The next version number for a file. Every change gets one, and it only goes up.
    pub(super) fn next_version(&self, path: &Path) -> i32 {
        let mut versions = self.inner.versions.borrow_mut();
        let version = versions.entry(path.to_path_buf()).or_insert(0);
        *version += 1;
        *version
    }

    /// Takes one thing a server said. Answers whether anything a view draws has changed.
    pub(super) fn absorb(&self, notice: Notice) -> bool {
        match notice {
            Notice::Diagnostics {
                uri,
                diagnostics,
                version,
            } => {
                let Some(path) = zdt_lsp::convert::path_of(&uri) else {
                    return false;
                };
                // A diagnostic about a version that has been typed past points at text that has
                // moved. Dropping it is better than drawing it in the wrong place.
                if let Some(version) = version
                    && let Some(current) = self.inner.versions.borrow().get(&path)
                    && version < *current
                {
                    return false;
                }
                // Which server said it: the first one answering for the file. A publish carries
                // no server name, so this is the best that can be known. It is right whenever one
                // server answers for a file, which is almost always.
                let server = self
                    .servers_for(&path)
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "lsp".to_owned());
                self.inner
                    .store
                    .borrow_mut()
                    .set(&path, &server, diagnostics);
                true
            }
            Notice::Message {
                server,
                severity,
                text,
            } => {
                // A server talking unprompted is news, so it goes to the stack. Two servers with
                // something to say make two rows. One slot would show the last of them.
                if let Some(notify) = self.inner.notify.as_ref() {
                    match severity {
                        lsp_types::MessageType::ERROR => notify.fail(server, Some(text)),
                        lsp_types::MessageType::WARNING => notify.warn(format!("{server}: {text}")),
                        _ => notify.say(format!("{server}: {text}")),
                    }
                }
                false
            }
            Notice::Progress {
                server,
                title,
                done,
            } => {
                let now = if done {
                    None
                } else {
                    Some(match title.as_deref() {
                        Some(title) => format!("{server}: {title}"),
                        None => server.clone(),
                    })
                };

                let before = self.inner.busy.get_untracked();
                if before == now {
                    // The same message again, which most reports are. Nothing has changed, so
                    // nothing is redrawn.
                    return false;
                }

                // One toast for the whole job, not one per report.
                //
                // `rust-analyzer` reports once per crate it indexes, which is thousands of
                // notices in a few seconds on a workspace of any size. A toast per report mounts
                // a component, an expiry timer and two animations per crate, and leaves every
                // dismissed one on the stack until its exit finishes. That is enough work to stop
                // the window answering the keyboard at all.
                //
                // So the toast is pushed when the server *starts* being busy and taken away when
                // it stops. What it is busy *with* changes constantly and lives in the status
                // line, which costs one signal write.
                match (before.is_some(), now.is_some()) {
                    (false, true) => self.announce(
                        &server,
                        Toast::new(title.clone().unwrap_or_else(|| "indexing".to_owned()))
                            .description(server.clone())
                            .kind(ToastKind::Loading)
                            .persistent(),
                    ),
                    (true, false) => self.forget_announcement(&server),
                    _ => {}
                }

                self.inner.busy.set(now);
                // Setting `busy` is what re-runs the status line; the revision is for the things
                // that draw diagnostics, and none of those has moved.
                false
            }
            Notice::Exited { server } => {
                self.inner.pool.borrow_mut().exited(&server);
                self.inner.store.borrow_mut().forget_server(&server);
                self.announce(
                    &server,
                    Toast::new(format!("{server} has stopped"))
                        .kind(ToastKind::Error)
                        .persistent(),
                );
                true
            }
        }
    }

    /// Says something about `server`, in the one row that server owns.
    ///
    /// Silent when nothing is listening, which is every test that mounts the language layer
    /// without a toaster over it.
    fn announce(&self, server: &str, toast: Toast) {
        if let Some(notify) = self.inner.notify.as_ref() {
            notify.progress(server, toast);
        }
    }

    /// Gives `server`'s row back.
    pub(super) fn forget_announcement(&self, server: &str) {
        if let Some(notify) = self.inner.notify.as_ref() {
            notify.clear(server);
        }
    }

    /// Says that something a view draws has changed.
    pub(super) fn touch(&self) {
        self.inner
            .revision
            .update(|revision| *revision = revision.wrapping_add(1));
    }
}
