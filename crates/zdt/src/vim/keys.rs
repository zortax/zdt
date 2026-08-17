//! One key, and where it goes.

use super::*;

impl Vim {
    /// Puts the engine back in normal mode, which a buffer or window switch has to do.
    ///
    /// The editor being left takes its visual painting off with it: nothing is selected in a mode
    /// nobody is in any more.
    pub fn reset(&self) {
        self.inner.engine.borrow_mut().reset();
        if let Some(handle) = self.inner.workspace.current_handle() {
            handle.set_overlay(Overlay::default());
        }
        self.publish();
    }

    /// Takes one key. Answers whether the editor should be left out of it.
    ///
    /// This is what an editor's key filter is: `true` means the key is used up.
    pub fn key(&self, chord: Chord, handle: &EditorHandle) -> bool {
        // Documentation has two states, and they take keys differently.
        //
        // Focused, which a second `K` does, it takes every key it has a row for. Somebody who
        // asked twice is reading it. Merely showing, it takes none and closes on the next key,
        // whatever that key is. A panel that swallowed keys would have to be dismissed before
        // work could continue.
        if let Some(hover) = zgui::reactive::use_local_context::<crate::hover::Hover>()
            && hover.is_showing()
        {
            if hover.is_focused() {
                return self.key_in_region(chord, "hover");
            }
            // The same key again means "I want to read this", so it takes the keyboard rather
            // than dismissing what it just opened. Asked here and not in the action, because by
            // then this branch has already closed the panel.
            if self.chord_runs(chord, "lsp.hover") && hover.focus() {
                return true;
            }
            hover.hide();
        }

        // Suggestions take the keys they are bound to and nothing else, so typing on past a
        // popup is typing. Before the grammar, because `<CR>` in insert mode means "take this
        // one" while the popup is up.
        if let Some(completion) =
            zgui::reactive::use_local_context::<crate::completion::Completion>()
            && completion.is_open()
            && self.mode_untracked() == Mode::Insert
            && self.key_in_region_as(chord, "completion", Mode::Insert)
        {
            return true;
        }

        // Labelled tabs take the next key, whatever it is: every letter is a label or the end of
        // the labelling, so a keymap answering one would put some tabs out of reach.
        if let Some(tabs) = zgui::reactive::use_local_context::<crate::tabpick::TabPick>()
            && tabs.is_running()
        {
            let character = match chord.key {
                zdt_vim::chord::Key::Char(character) if chord.mods.is_empty() => Some(character),
                _ => None,
            };
            return tabs.key(character);
        }

        // A leap in progress takes every key: once it has started, each one is either a character
        // it is aiming at or a label, and a keymap that answered any of them would put some
        // letters out of reach.
        if self.inner.leaping.is_running() {
            return self.leap_key(chord, handle);
        }

        let step = self.step(chord, handle);
        self.publish();
        match step {
            Step::Consumed(effects) => {
                self.carry_out(effects, handle);
                true
            }
            Step::Pending => true,
            Step::PassThrough => false,
        }
    }

    /// Starts a leap looking `direction`.
    pub fn start_leap(&self, direction: zdt_vim::leap::Direction) {
        self.inner.leaping.start(direction);
    }

    /// The leap layer, for the overlay that draws its labels.
    #[must_use]
    pub fn leaping(&self) -> crate::leap::Leaping {
        self.inner.leaping.clone()
    }

    /// One key while a leap is in progress.
    ///
    /// Always answers `true`: every key belongs to the leap while one is running, including the
    /// one that ends it.
    fn leap_key(&self, chord: Chord, handle: &EditorHandle) -> bool {
        use zdt_vim::chord::Key;

        // Only a plain character narrows or chooses. A chord with a modifier on it ends the
        // leap, and so does `<Esc>`, which is how anybody expects to get out.
        let character = match chord.key {
            Key::Char(character) if chord.mods.is_empty() => Some(character),
            _ => None,
        };

        let took = handle.query(|snapshot| {
            let rope = snapshot.rope();
            let window = snapshot.visible_byte_range();
            let caret = snapshot.selections().primary().head;
            self.inner
                .leaping
                .key(character, |pair, direction, alphabet| {
                    zdt_vim::leap::landings(rope, window, caret, pair, direction, alphabet)
                })
        });

        if let crate::leap::Took::Landed(byte) = took {
            let step = handle.query(|snapshot| {
                let selections: Vec<Selection> = snapshot
                    .selections()
                    .iter()
                    .map(|selection| Selection::new(selection.anchor, selection.head))
                    .collect();
                let visible = snapshot.visible_lines();
                let context = Context {
                    rope: snapshot.rope(),
                    selections: &selections,
                    view: View {
                        top_line: visible.start,
                        height: visible.len().max(1),
                    },
                };
                self.inner.engine.borrow_mut().leap_to(byte, &context)
            });
            if let Step::Consumed(effects) = step {
                self.carry_out(effects, handle);
            }
        }
        self.publish();
        true
    }

    /// Takes one key for a region that is not an editor: the tree, a picker, a terminal.
    ///
    /// The same keymap with that region's rows in front, resolved in normal mode, and with no
    /// editor to apply anything to. A region has no modes of its own. Answers whether the key was
    /// used.
    pub fn key_in_region(&self, chord: Chord, region: &str) -> bool {
        self.key_in_region_as(chord, region, Mode::Normal)
    }

    /// The same, resolved in `mode`.
    ///
    /// For a terminal being typed into. Almost nothing is bound in terminal mode, so almost every
    /// key falls through to the program, which is the point. What *is* bound there is what vim's
    /// own `maps.t` binds, and it wins over the program on purpose.
    pub fn key_in_region_as(&self, chord: Chord, region: &str, mode: Mode) -> bool {
        let overlay = self.inner.overlays.borrow();
        let map = overlay.get(region);
        let keymap = self.inner.keymap.borrow();
        // A region with no keymap of its own still gets the base map: a terminal in normal mode
        // has no rows of its own, and `<Leader>ff` from inside one has to work.
        let layered = match map {
            Some(map) => Layered::new(map, &keymap),
            None => Layered::plain(&keymap),
        };

        // A region's keys have no grammar: no counts, no operators, nothing to hold between
        // presses but the sequence itself.
        let mut keys = self.inner.region_keys.borrow_mut();
        keys.push(chord);

        match layered.resolve(mode, &keys) {
            Resolution::Pending(_) => {
                drop(keys);
                self.publish_region();
                true
            }
            Resolution::None => {
                keys.clear();
                drop(keys);
                self.publish_region();
                false
            }
            Resolution::Run(binding) => {
                let actions = binding.actions.clone();
                keys.clear();
                drop(keys);
                drop(overlay);
                drop(keymap);
                self.publish_region();
                for action in &actions {
                    crate::actions::run(&self.inner.workspace, self, action, None);
                }
                true
            }
        }
    }

    /// Whether `chord` on its own runs `action` in the base map, in whatever mode this is.
    ///
    /// For the places that have to know what a key *means* before deciding whether to let it
    /// through. Against the base map, so it follows a person's own keymap.
    #[must_use]
    pub fn chord_runs(&self, chord: Chord, action: &str) -> bool {
        let keymap = self.inner.keymap.borrow();
        match Layered::plain(&keymap).resolve(self.mode_untracked(), &[chord]) {
            Resolution::Run(binding) => binding.actions.iter().any(|one| one.name == action),
            _ => false,
        }
    }

    /// Echoes what a region has typed so far, so which-key and the status line follow it too.
    fn publish_region(&self) {
        let keys = self.inner.region_keys.borrow();
        let pending = zdt_vim::notation::format(&keys);
        if self.inner.pending.get_untracked() != pending {
            self.inner.pending.set(pending);
        }
    }
}
