//! What the interface reads.

use super::*;

impl Vim {
    /// Which mode the editor is in. Tracked.
    pub fn mode(&self) -> Mode {
        self.inner.mode.get()
    }

    /// Which mode the editor is in, without subscribing.
    pub fn mode_untracked(&self) -> Mode {
        self.inner.mode.get_untracked()
    }

    /// What has been typed toward a binding that has not resolved. Tracked.
    pub fn pending(&self) -> String {
        self.inner.pending.get()
    }

    /// Which register a macro is being recorded into. Tracked.
    pub fn recording(&self) -> Option<char> {
        self.inner.recording.get()
    }

    /// What could come next, when a sequence is part-way through. Tracked.
    ///
    /// Answered as owned rows. The keymap is behind a `RefCell`, and which-key draws from a
    /// reactive hole, which is no place to hold a borrow.
    ///
    /// Untracked: what wakes which-key is the pending signal, which the panel watches for itself.
    pub fn continuations(&self) -> Vec<Continuation> {
        let engine = self.inner.engine.borrow();
        let keys = engine.pending_keys();
        // An operator waiting for something to apply to has typed nothing yet, and the motions are
        // exactly what somebody who paused after `d` is looking for.
        if keys.is_empty() && engine.pending_operator().is_none() {
            return Vec::new();
        }

        let keymap = self.inner.keymap.borrow();
        let layered = Layered::plain(&keymap);

        match layered.resolve(engine.mode(), keys) {
            Resolution::Pending(next) => next
                .into_iter()
                .map(|one| Continuation {
                    keys: zdt_vim::notation::format(&[one.chord]),
                    label: one.label.to_owned(),
                    runs: one.runs,
                })
                .collect(),
            Resolution::Run(_) | Resolution::None => Vec::new(),
        }
    }

    /// Everything bound in normal mode, as the keys that reach it and what it is called.
    ///
    /// Owned, because the keymap is behind a `RefCell` and a picker holds what it lists for as
    /// long as it is open.
    #[must_use]
    pub fn bindings(&self) -> Vec<Bound> {
        let keymap = self.inner.keymap.borrow();
        keymap
            .bindings(Mode::Normal)
            .into_iter()
            .map(|(keys, binding)| Bound {
                keys: zdt_vim::notation::format(&keys),
                actions: binding.actions.clone(),
                description: binding.description.clone(),
            })
            .collect()
    }

    /// What is in each register that has anything in it.
    #[must_use]
    pub fn registers(&self) -> Vec<(char, String)> {
        let engine = self.inner.engine.borrow();
        engine
            .registers()
            .occupied()
            .into_iter()
            .map(|(name, contents)| (name, contents.text.clone()))
            .collect()
    }

    /// Where each mark is, as a byte offset into the buffer it was set in.
    #[must_use]
    pub fn marks(&self) -> Vec<(char, usize)> {
        self.inner.engine.borrow().marks()
    }

    /// Carries out one action, as though a key had asked for it.
    ///
    /// What a picker of commands does with the row somebody chose.
    pub fn run(&self, action: &zdt_vim::Action) {
        let handle = self.inner.workspace.current_handle();
        crate::actions::run(&self.inner.workspace, self, action, handle.as_ref());
    }
}
