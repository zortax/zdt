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
        // A region is asked first, and on its own terms. Its keys have no grammar and never reach
        // the engine, so the engine's pending keys say nothing about a `g` typed in the file tree.
        //
        // The guard is what it has typed, and not the marker: once a region's sequence resolves
        // its keys are cleared, and which-key falls back to the engine without anything having to
        // remember to say so.
        let typed = self.inner.region_keys.borrow();
        if let Some(typing) = self.inner.typing.get()
            && !typed.is_empty()
        {
            return self.next_after(Some(typing.region), typing.mode, &typed);
        }
        drop(typed);

        let engine = self.inner.engine.borrow();
        let keys = engine.pending_keys();
        // An operator waiting for something to apply to has typed nothing yet, and the motions are
        // exactly what somebody who paused after `d` is looking for.
        if keys.is_empty() && engine.pending_operator().is_none() {
            return Vec::new();
        }
        self.next_after(None, engine.mode(), keys)
    }

    /// What could continue `keys` in `region`, resolved in `mode`.
    fn next_after(&self, region: Option<&str>, mode: Mode, keys: &[Chord]) -> Vec<Continuation> {
        self.inner
            .keymaps
            .with_layered(region, |layered| match layered.resolve(mode, keys) {
                Resolution::Pending(next) => next
                    .into_iter()
                    .map(|one| Continuation {
                        keys: zdt_vim::notation::format(&[one.chord]),
                        label: one.label.to_owned(),
                        runs: one.runs,
                    })
                    .collect(),
                Resolution::Run(_) | Resolution::None => Vec::new(),
            })
    }

    /// Everything bound in normal mode, as the keys that reach it and what it is called.
    ///
    /// Owned, because the keymap is behind a `RefCell` and a picker holds what it lists for as
    /// long as it is open.
    #[must_use]
    pub fn bindings(&self) -> Vec<Bound> {
        self.inner.keymaps.with_base(|keymap| -> Vec<Bound> {
            keymap
                .bindings(Mode::Normal)
                .into_iter()
                .map(|(keys, binding)| Bound {
                    keys: zdt_vim::notation::format(&keys),
                    actions: binding.actions.clone(),
                    description: binding.description.clone(),
                })
                .collect()
        })
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
    pub fn marks(&self) -> Vec<(char, zdt_vim::Place)> {
        self.inner.engine.borrow().marks()
    }

    /// Where the caret has been, and how far back through it `<C-o>` has walked.
    #[must_use]
    pub fn jumps(&self) -> (Vec<zdt_vim::Place>, usize) {
        self.inner.engine.borrow().jumps()
    }

    /// Every register that holds anything, with the unnamed one under an empty name.
    ///
    /// For writing a session down, which wants the linewise flag as well as the text.
    #[must_use]
    pub fn registers_full(&self) -> Vec<(String, String, bool)> {
        let engine = self.inner.engine.borrow();
        let registers = engine.registers();
        let mut found: Vec<(String, String, bool)> = registers
            .occupied()
            .into_iter()
            .map(|(name, contents)| (name.to_string(), contents.text.clone(), contents.linewise))
            .collect();
        let unnamed = registers.unnamed();
        if !unnamed.text.is_empty() {
            found.push((String::new(), unnamed.text.clone(), unnamed.linewise));
        }
        found
    }

    /// Puts vim's memory back, which restoring a session does.
    pub fn restore_memory(
        &self,
        registers: Vec<(String, String, bool)>,
        marks: Vec<(char, zdt_vim::Place)>,
        jumps: Vec<zdt_vim::Place>,
        jump_at: usize,
    ) {
        let mut engine = self.inner.engine.borrow_mut();
        let mut unnamed = zdt_vim::register::Contents::default();
        let mut held: Vec<(char, zdt_vim::register::Contents)> = Vec::new();
        for (name, text, linewise) in registers {
            let contents = zdt_vim::register::Contents { text, linewise };
            match name.chars().next() {
                Some(character) => held.push((character, contents)),
                None => unnamed = contents,
            }
        }
        engine.registers_mut().restore(unnamed, held);
        engine.set_marks(marks);
        engine.set_jumps(jumps, jump_at);
    }

    /// Which buffer the modal layer is acting on, as the engine names buffers.
    ///
    /// A mark and a jump carry this so that they say *where* as well as *how far in*. The engine
    /// never interprets it.
    #[must_use]
    pub fn owner(&self) -> zdt_vim::Owner {
        use slotmap::Key;
        self.inner
            .workspace
            .current_buffer()
            .map_or(zdt_vim::Owner::default(), |buffer| {
                zdt_vim::Owner(buffer.id.data().as_ffi())
            })
    }

    /// Shows the buffer `place` is in, and puts the caret there.
    ///
    /// What a mark or a jump into another buffer asks for. A buffer that has since been closed is
    /// said out loud rather than silently doing nothing.
    pub fn go_to(&self, place: zdt_vim::Place) {
        use slotmap::Key;
        let workspace = &self.inner.workspace;
        let wanted = workspace
            .order()
            .into_iter()
            .find(|id| id.data().as_ffi() == place.owner.0);
        let Some(id) = wanted else {
            workspace.complain("that buffer is not open any more");
            return;
        };
        workspace.show(id);
        // After the switch, so the editor being written to is the one now on screen.
        if let Some(handle) = workspace.handle_for(workspace.focused_untracked(), id) {
            let byte = handle.query(|snapshot| place.byte.min(snapshot.rope().len_bytes()));
            handle.command(zgui_editor::Command::SetSelections {
                selections: vec![zgui_editor::Selection::new(byte, byte)],
                primary: 0,
            });
            handle.command(zgui_editor::Command::Scroll(
                zgui_editor::ScrollCmd::CursorCenter,
            ));
        }
    }

    /// Carries out one action, as though a key had asked for it.
    ///
    /// What a picker of commands does with the row somebody chose.
    pub fn run(&self, action: &zdt_vim::Action) {
        let handle = self.inner.workspace.current_handle();
        crate::actions::run(&self.inner.workspace, self, action, handle.as_ref());
    }
}
