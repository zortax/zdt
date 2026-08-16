//! Gathering candidates, and ranking them.

use super::*;

impl Picker {
    // ---- Gathering ---------------------------------------------------------------------------

    /// Puts whatever `source` picks from where the ranking can reach it.
    pub(super) fn gather(&self, source: &Source, query: &str) {
        match source {
            Source::Given { rows, .. } => self.stand(rows.clone(), query),
            Source::WorkspaceSymbols => self.start_symbols(query),
            Source::Files { reach } => self.gather_files(*reach),
            Source::Grep { .. } => self.start_grep(source, query),
            Source::Buffers => self.stand(self.buffers(), query),
            Source::Lines => self.stand(self.lines(), query),
            Source::Themes => self.stand(self.themes(), query),
            Source::Commands | Source::Keymaps => {
                self.stand(self.bindings(matches!(source, Source::Keymaps)), query);
            }
            Source::Recent => self.stand(self.recent(), query),
            Source::Registers => self.stand(self.registers(), query),
            Source::Marks => self.stand(self.marks(), query),
            Source::GitFiles => self.gather_git(query),
        }
    }

    /// Takes a gathered list and ranks it for the first time.
    pub(super) fn stand(&self, rows: Vec<Row>, query: &str) {
        *self.inner.candidates.borrow_mut() = rows;
        self.rank(query);
    }

    /// The project's files, walked on a worker and handed to the matcher.
    fn gather_files(&self, reach: crate::picker::source::Reach) {
        let generation = self.inner.generation.get();
        let root = self.inner.workspace.project().root().to_path_buf();
        let (ignored, hidden) = self
            .inner
            .settings
            .with_untracked(|config| (config.picker.ignored, config.picker.hidden));
        let walk = Walk {
            ignored: reach.ignored || ignored,
            hidden: reach.hidden || hidden,
            ..Walk::default()
        };

        self.inner.working.set(true);
        let picker = self.clone();
        zdt_view::detached(async move {
            let walked = {
                let root = root.clone();
                zgui::task::blocking(move || zdt_core::search::files::walk(&root, walk)).await
            };
            if picker.inner.generation.get() != generation {
                return;
            }

            // No wake: this polls on a timer of its own, so being told there is something
            // new would only ask for a frame that is coming anyway.
            let mut ranker = Ranker::new(|| {});
            ranker.fill(walked);
            // What has been typed *now*, not what had been when the walk started: a walk over a
            // large project takes long enough to type a word into, and that word must not be lost.
            ranker.seek(&picker.inner.query.get_untracked());
            *picker.inner.ranker.borrow_mut() = Some(ranker);
            picker.poll_ranker();
        });
    }

    /// Ranks the standing candidates, or asks the matcher to.
    pub(super) fn rank(&self, query: &str) {
        // A file list still being walked has neither: leaving the rows alone is right, because
        // the walk reads what has been typed when it lands.
        if self.inner.ranker.borrow().is_none()
            && matches!(
                self.inner.source.get_untracked(),
                Some(Source::Files { .. } | Source::GitFiles)
            )
            && self.inner.candidates.borrow().is_empty()
        {
            return;
        }

        if self.inner.ranker.borrow().is_some() {
            if let Some(ranker) = self.inner.ranker.borrow_mut().as_mut() {
                ranker.seek(query);
            }
            self.poll_ranker();
            return;
        }

        let limit = self
            .inner
            .settings
            .with_untracked(|config| config.picker.max_results);
        let candidates = self.inner.candidates.borrow();
        let labels: Vec<String> = candidates.iter().map(|row| row.label.clone()).collect();
        let ranked = zdt_core::search::fuzzy::rank(&labels, query, limit);
        let rows: Vec<Row> = ranked
            .into_iter()
            .filter_map(|held| {
                candidates
                    .get(held.index)
                    .cloned()
                    .map(|row| row.with_matched(held.matched))
            })
            .collect();
        let total = candidates.len();
        drop(candidates);

        self.inner.counts.set((rows.len(), total));
        self.publish(rows);
        self.preview_row();
    }

    /// Keeps asking the matcher for its answer until it says it has finished.
    fn poll_ranker(&self) {
        let Some(timers) = self.inner.timers.clone() else {
            return;
        };
        let generation = self.inner.generation.get();
        // The matcher says it has stopped before it has started. The items are pushed from here
        // and picked up by its threads a moment later, so the first tick answers "nothing to do"
        // about work that has not begun. Several quiet ticks tell a matcher that has finished from
        // one that has not begun.
        let quiet = Cell::new(0_u8);
        let picker = self.clone();
        let handle = timers.set_interval(POLL, move || {
            if picker.inner.generation.get() != generation {
                return;
            }
            let limit = picker
                .inner
                .settings
                .with_untracked(|config| config.picker.max_results);

            let (progress, matched, counts) = {
                let mut held = picker.inner.ranker.borrow_mut();
                let Some(ranker) = held.as_mut() else {
                    return;
                };
                let progress = ranker.poll();
                let matched = progress.changed.then(|| ranker.matches(limit));
                (progress, matched, ranker.counts())
            };

            let moved =
                picker.inner.counts.get_untracked() != (counts.0 as usize, counts.1 as usize);
            if let Some(matched) = matched.or_else(|| {
                // The counts moved without the matcher calling it a change: the items arrived.
                moved.then(|| {
                    let limit = picker
                        .inner
                        .settings
                        .with_untracked(|config| config.picker.max_results);
                    picker
                        .inner
                        .ranker
                        .borrow()
                        .as_ref()
                        .map(|ranker| ranker.matches(limit))
                        .unwrap_or_default()
                })
            }) {
                let root = picker.inner.workspace.project().root().to_path_buf();
                let rows: Vec<Row> = matched
                    .into_iter()
                    .map(|(path, landed)| Row::file(path, &root, None).with_matched(landed))
                    .collect();
                picker
                    .inner
                    .counts
                    .set((counts.0 as usize, counts.1 as usize));
                picker.publish(rows);
            }

            if progress.running || progress.changed || moved {
                quiet.set(0);
            } else {
                quiet.set(quiet.get() + 1);
            }
            if quiet.get() >= 4 {
                picker.inner.working.set(false);
                // Stopping from inside the callback. Dropping the handle cancels it, and the
                // picker holds the handle.
                *picker.inner.polling.borrow_mut() = None;
            }
        });
        self.inner.working.set(true);
        *self.inner.polling.borrow_mut() = Some(handle);
    }
}
