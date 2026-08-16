//! Searching file contents, and searching a project's symbols.

use super::*;

impl Picker {
    // ---- Grep --------------------------------------------------------------------------------

    /// Starts a search after a pause, cancelling whatever was running.
    ///
    /// What is on screen stays there until the new search has something to put in its place.
    /// Clearing on the keystroke empties the list and the preview for as long as the search takes,
    /// which at one search per keystroke is a flicker the whole time somebody is typing.
    pub(super) fn start_grep(&self, source: &Source, query: &str) {
        self.stop();
        if query.is_empty() {
            self.publish(Vec::new());
            self.inner.counts.set((0, 0));
            return;
        }
        self.inner.stale.set(true);

        let Some(timers) = self.inner.timers.clone() else {
            return;
        };
        let reach = match source {
            Source::Grep { reach, .. } => *reach,
            _ => crate::picker::source::Reach::default(),
        };
        let query = query.to_owned();
        let picker = self.clone();
        let handle = timers.set_timeout(GREP_DEBOUNCE, move || picker.run_grep(reach, &query));
        *self.inner.pending.borrow_mut() = Some(handle);
        self.inner.working.set(true);
    }

    /// Asks the language servers what in the project matches, after a pause.
    ///
    /// Live for the same reason grep is. No server will list every symbol in a project of ten
    /// thousand files, and none should be asked to. The query is the request, so each keystroke
    /// cancels the one before it. The generation moves, because a request already sent cannot be
    /// recalled and its answer can only be dropped.
    pub(super) fn start_symbols(&self, query: &str) {
        self.stop();
        if query.is_empty() {
            self.publish(Vec::new());
            self.inner.counts.set((0, 0));
            return;
        }
        self.inner.stale.set(true);

        let Some(timers) = self.inner.timers.clone() else {
            return;
        };
        // The language layer is taken *now*, before the timer. A context looked up inside a
        // timer callback is gone, and the search would silently find nothing. See
        // `tests/context.rs`.
        let language = zgui::reactive::use_local_context::<crate::language::Language>();
        let query = query.to_owned();
        let picker = self.clone();
        let handle =
            timers.set_timeout(GREP_DEBOUNCE, move || picker.run_symbols(&query, language));
        *self.inner.pending.borrow_mut() = Some(handle);
        self.inner.working.set(true);
    }

    /// Asks, now.
    fn run_symbols(&self, query: &str, language: Option<crate::language::Language>) {
        let generation = self.inner.generation.get();
        let root = self.inner.workspace.project().root().to_path_buf();

        let Some(language) = language else {
            self.inner.working.set(false);
            return;
        };
        let Some(mut client) = language
            .current_path()
            .and_then(|path| language.client_for(&path))
        else {
            self.inner.working.set(false);
            self.inner.workspace.say("no language server for this file");
            return;
        };

        let query = query.to_owned();
        let picker = self.clone();
        zdt_view::detached(async move {
            let found = {
                let query = query.clone();
                zgui::task::background(async move { client.workspace_symbols(&query).await }).await
            };
            // An answer for a question nobody is asking any more.
            if picker.inner.generation.get() != generation {
                return;
            }
            picker.inner.working.set(false);

            match found {
                Ok(symbols) => {
                    let rows = symbol_rows(&symbols, &root);
                    picker.inner.counts.set((rows.len(), rows.len()));
                    picker.publish(rows);
                }
                Err(error) => picker.inner.workspace.complain(error.to_string()),
            }
        });
    }

    /// Runs one search, reporting its hits in batches.
    fn run_grep(&self, reach: crate::picker::source::Reach, pattern: &str) {
        let generation = self.inner.generation.get();
        let root = self.inner.workspace.project().root().to_path_buf();
        let (limit, smart_case) = self
            .inner
            .settings
            .with_untracked(|config| (config.picker.max_results, config.picker.smart_case));
        let query = zdt_core::search::grep::Query {
            pattern: pattern.to_owned(),
            regex: false,
            smart_case,
            walk: Walk {
                ignored: reach.ignored,
                hidden: reach.hidden,
                ..Walk::default()
            },
            limit: limit.max(1) * 4,
        };

        let cancel = Cancel::new();
        *self.inner.cancel.borrow_mut() = Some(cancel.clone());

        // Hits come back down a channel, and never through a posted closure. They are found on
        // the walk's own threads, and everything on this side of the picker is `Rc` and belongs to
        // the interface thread. A channel is the one shape that needs nothing of either.
        let (sender, receiver) = std::sync::mpsc::channel::<Vec<zdt_core::search::Hit>>();
        self.drain_hits(receiver, generation, limit, root.clone());

        let picker = self.clone();
        zdt_view::detached(async move {
            let outcome = {
                let (root, query, cancel) = (root.clone(), query.clone(), cancel.clone());
                zgui::task::blocking(move || {
                    zdt_core::search::grep::search(&root, &query, &cancel, |batch| {
                        // A closed channel means nobody is listening any more, which is not worth
                        // saying: the search is about to be cancelled for the same reason.
                        let _ = sender.send(batch);
                    })
                })
                .await
            };
            if picker.inner.generation.get() != generation {
                return;
            }
            picker.inner.working.set(false);
            // Nothing was found, so nothing replaced the last search's rows: they go now. Waiting
            // until the end to say so is what keeps them on screen while there is still hope.
            if picker.inner.stale.replace(false) {
                picker.publish(Vec::new());
                picker.inner.counts.set((0, 0));
            }
            if let Err(error) = outcome {
                picker.inner.workspace.complain(error.to_string());
            }
        });
    }

    /// Takes whatever the search has found and puts it on the screen, a few times a second.
    ///
    /// Batched. A grep over a large repository finds thousands of hits, and a signal written per
    /// hit would be thousands of frames.
    fn drain_hits(
        &self,
        receiver: std::sync::mpsc::Receiver<Vec<zdt_core::search::Hit>>,
        generation: u64,
        limit: usize,
        root: std::path::PathBuf,
    ) {
        let Some(timers) = self.inner.timers.clone() else {
            return;
        };
        let picker = self.clone();
        let handle = timers.set_interval(POLL, move || {
            if picker.inner.generation.get() != generation {
                return;
            }
            let rows: Vec<Row> = receiver
                .try_iter()
                .flatten()
                .map(|hit| {
                    Row::file(hit.path, &root, Some(hit.line))
                        .with_detail(hit.text.trim_start().to_owned())
                        .with_match(hit.column..hit.column + hit.length)
                })
                .collect();
            picker.extend(rows, limit);

            // The walk has finished and the channel is empty: there is nothing left to drain.
            if !picker.inner.working.get_untracked() {
                *picker.inner.polling.borrow_mut() = None;
            }
        });
        *self.inner.polling.borrow_mut() = Some(handle);
    }
}
