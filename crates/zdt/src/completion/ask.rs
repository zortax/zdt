//! Asking the server, and taking what it answers.

use super::*;

impl Completion {
    // ---- Asking ------------------------------------------------------------------------------

    /// Asks the server what could be typed, now.
    ///
    /// What `<C-Space>` does, and what the debounce below comes to. Nothing happens when the caret
    /// is not in a word and no trigger character was typed: a popup over an empty line is a popup
    /// listing the whole crate.
    pub fn ask(&self, workspace: &Workspace, handle: Option<&EditorHandle>) {
        let Some(handle) = handle.cloned() else {
            return;
        };
        let Some(language) = self.inner.language.clone() else {
            return;
        };
        let Some(path) = workspace.current_buffer().and_then(|buffer| buffer.path) else {
            return;
        };
        let Some(mut client) = language.client_for(&path) else {
            return;
        };

        let prefix = prefix_at(&handle);
        let (query, replaces) = match prefix {
            Some((word, range)) => (word, range),
            // Outside a word. The popup replaces nothing and the query is empty, which is what
            // a trigger character such as a dot or a colon means.
            None => {
                let caret = handle.query(|snapshot| snapshot.selections().primary().head);
                (String::new(), caret..caret)
            }
        };

        let generation = self.inner.generation.get() + 1;
        self.inner.generation.set(generation);
        self.inner.asked_at.set(replaces.start);

        let position = handle.query(|snapshot| {
            zdt_lsp::convert::position_of(snapshot.rope(), replaces.end, client.encoding)
        });
        let Some(caret) = handle.point_for_byte(replaces.end) else {
            // Off screen: there is nowhere to draw a popup, so there is no point asking.
            return;
        };

        let completion = self.clone();
        zdt_view::detached(async move {
            let found = {
                let path = path.clone();
                zgui::task::background(async move { client.completion(&path, position).await })
                    .await
            };
            // An answer for a question nobody is asking any more.
            if completion.inner.generation.get() != generation {
                return;
            }
            match found {
                Ok(items) if items.is_empty() => completion.close(),
                Ok(items) => completion.arrived(items, &query, replaces, caret),
                // Silently: a completion that could not be fetched is a completion that does not
                // appear, and a toast for every failed keystroke would be unusable.
                Err(error) => {
                    tracing::debug!("completion: {error}");
                    completion.close();
                }
            }
        });
    }

    /// Asks after a pause, which typing does.
    ///
    /// Called from the editor's own event stream, so it must be cheap when the answer is "no":
    /// everything that decides against asking is done before a timer is started.
    pub fn typed(&self, workspace: &Workspace, handle: Option<&EditorHandle>) {
        let (wanted, least) = self.inner.settings.with_untracked(|config| {
            (config.editor.completion, config.editor.completion_min_chars)
        });
        if !wanted {
            return;
        }
        let Some(handle) = handle else {
            self.close();
            return;
        };

        match prefix_at(handle) {
            // Still inside the word the list was fetched for: re-rank what is already here rather
            // than asking again. This is what makes typing inside a word cost nothing.
            Some((word, range)) if self.is_open() && range.start == self.inner.asked_at.get() => {
                self.refilter(&word, range);
                return;
            }
            Some((word, _)) if word.chars().count() >= least.max(1) => {}
            // Too short to ask about, or not in a word at all.
            _ => {
                self.close();
                return;
            }
        }

        let Some(timers) = self.inner.timers.clone() else {
            self.ask(workspace, Some(handle));
            return;
        };
        let (completion, workspace, handle) = (self.clone(), workspace.clone(), handle.clone());
        let waiting = timers.set_timeout(DEBOUNCE, move || {
            completion.inner.pending.borrow_mut().take();
            completion.ask(&workspace, Some(&handle));
        });
        // Replacing the handle cancels the one before it, which is the debounce.
        *self.inner.pending.borrow_mut() = Some(waiting);
    }

    /// Takes what the server sent.
    fn arrived(
        &self,
        items: Vec<lsp_types::CompletionItem>,
        query: &str,
        replaces: std::ops::Range<usize>,
        caret: zgui_editor::CaretRect,
    ) {
        let ranked = rank(&items, query);
        if ranked.is_empty() {
            self.close();
            return;
        }
        *self.inner.all.borrow_mut() = items;
        self.inner.items.set(ranked);
        self.inner.at.set(0);
        self.forget_docs();
        self.inner.open.set(Some(Open { caret, replaces }));
        self.want_docs();
    }

    /// Ranks what is already here against a longer prefix.
    fn refilter(&self, query: &str, replaces: std::ops::Range<usize>) {
        let ranked = {
            let all = self.inner.all.borrow();
            rank(&all, query)
        };
        if ranked.is_empty() {
            // Nothing matches what is being typed, so the popup closes. The box answers "what
            // could this be", and it answers "nothing" by getting out of the way.
            self.close();
            return;
        }
        let same = self.inner.items.with_untracked(|held| *held == ranked);
        if !same {
            self.inner.items.set(ranked);
            self.inner.at.set(0);
            self.forget_docs();
            self.want_docs();
        }
        // The range grows as the word does, so accepting replaces all of what was typed.
        self.inner.open.update(|open| {
            if let Some(open) = open.as_mut() {
                open.replaces = replaces;
            }
        });
    }

    /// Puts the row the caret is on into the buffer.
    pub fn accept(&self, handle: Option<&EditorHandle>) {
        let Some(handle) = handle else {
            self.close();
            return;
        };
        let Some(open) = self.inner.open.get_untracked() else {
            return;
        };
        let at = self.inner.at.get_untracked();
        let Some(row) = self
            .inner
            .items
            .with_untracked(|items| items.get(at).cloned())
        else {
            self.close();
            return;
        };
        let Some(item) = self.inner.all.borrow().get(row.index).cloned() else {
            self.close();
            return;
        };

        let encoding = self
            .inner
            .language
            .as_ref()
            .and_then(|language| {
                let path = language.path_of_handle(handle)?;
                language.client_for(&path).map(|client| client.encoding)
            })
            .unwrap_or_default();

        let (range, text) = replacement(&item, open.replaces.clone(), handle, encoding);

        // The item's own edit and everything else it asks for, in one command: an auto-import that
        // arrived as an additional edit has to land in the same undo step as the word it is for,
        // or undoing the completion leaves the import behind.
        let mut replacements = vec![(range, text)];
        if let Some(extra) = item.additional_text_edits.as_ref() {
            let mut more: Vec<(std::ops::Range<usize>, String)> = handle.query(|snapshot| {
                extra
                    .iter()
                    .map(|edit| {
                        (
                            zdt_lsp::convert::range_of(snapshot.rope(), edit.range, encoding),
                            edit.new_text.clone(),
                        )
                    })
                    .collect()
            });
            replacements.append(&mut more);
        }
        // Back to front, because every range is against the text as it is now.
        replacements.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
        handle.command(zgui_editor::Command::ReplaceRanges(replacements));

        self.close();
    }

    /// Asks for the documentation of the row the caret is on, after a pause.
    pub(super) fn want_docs(&self) {
        let Some(delay) = self.docs_delay() else {
            return;
        };
        let Some(timers) = self.inner.timers.clone() else {
            return;
        };
        let completion = self.clone();
        let waiting = timers.set_timeout(delay, move || {
            completion.inner.docs_pending.borrow_mut().take();
            completion.fetch_docs();
        });
        *self.inner.docs_pending.borrow_mut() = Some(waiting);
    }

    /// Asks for it, now.
    fn fetch_docs(&self) {
        let at = self.inner.at.get_untracked();
        let Some(row) = self
            .inner
            .items
            .with_untracked(|items| items.get(at).cloned())
        else {
            return;
        };
        let Some(item) = self.inner.all.borrow().get(row.index).cloned() else {
            return;
        };

        // What the server already sent, when it sent any: no round trip for a server that answers
        // in full the first time.
        if let Some(blocks) = documentation(&item) {
            self.inner.docs.set(Some(blocks));
            return;
        }

        let Some(language) = self.inner.language.clone() else {
            return;
        };
        let Some(path) = language.current_path() else {
            return;
        };
        let Some(mut client) = language.client_for(&path) else {
            return;
        };

        let generation = self.inner.docs_generation.get() + 1;
        self.inner.docs_generation.set(generation);

        let completion = self.clone();
        zdt_view::detached(async move {
            let found =
                zgui::task::background(async move { client.resolve_completion(item).await }).await;
            // Walking a list quickly must not draw the documentation of a row already left.
            if completion.inner.docs_generation.get() != generation {
                return;
            }
            if let Ok(resolved) = found
                && let Some(blocks) = documentation(&resolved)
            {
                completion.inner.docs.set(Some(blocks));
            }
        });
    }

    /// Forgets the documentation of whatever row was showing.
    pub(super) fn forget_docs(&self) {
        self.inner
            .docs_generation
            .set(self.inner.docs_generation.get() + 1);
        self.inner.docs_pending.borrow_mut().take();
        if self.inner.docs.with_untracked(Option::is_some) {
            self.inner.docs.set(None);
        }
        if self.inner.docs_offset.get_untracked() != 0.0 {
            self.inner.docs_offset.set(0.0);
        }
    }

    /// How long the caret rests on a row before its documentation is asked for, when it is at all.
    fn docs_delay(&self) -> Option<Duration> {
        self.inner.settings.with_untracked(|config| {
            config
                .editor
                .completion_doc
                .then(|| Duration::from_millis(config.editor.completion_doc_delay))
        })
    }
}
