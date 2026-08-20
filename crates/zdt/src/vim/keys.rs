//! One key, and where it goes.

use super::*;

/// What a region's key came to.
///
/// Decided while the keymaps are borrowed, acted on after they are not.
enum Region {
    /// Part of a longer sequence.
    Waiting,
    /// Bound to nothing, so the region does not want it.
    Unbound,
    /// What to run.
    Run(Vec<zdt_vim::Action>),
}

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
        //
        // Only one over the text. A leap over the file tree's rows answers a row number, which is
        // no place in a rope, and the tree takes its keys where its own keys arrive.
        if self.inner.leaping.is_running_over(crate::leap::Over::Text) {
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

    /// Starts a leap over the text, looking `direction`.
    pub fn start_leap(&self, direction: zdt_vim::leap::Direction) {
        self.inner.leaping.start(direction);
    }

    /// Starts one over something else, such as the rows of the file tree.
    ///
    /// Whichever region asked for it is the one that takes its keys, because a landing is a number
    /// only that region can read.
    pub fn start_leap_over(&self, direction: zdt_vim::leap::Direction, over: crate::leap::Over) {
        self.inner.leaping.start_over(direction, over);
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
                    owner: self.owner(),
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
    pub fn key_in_region(&self, chord: Chord, region: &'static str) -> bool {
        self.key_in_region_as(chord, region, Mode::Normal)
    }

    /// The same, resolved in `mode`.
    ///
    /// For a terminal being typed into. Almost nothing is bound in terminal mode, so almost every
    /// key falls through to the program, which is the point. What *is* bound there is what vim's
    /// own `maps.t` binds, and it wins over the program on purpose.
    pub fn key_in_region_as(&self, chord: Chord, region: &'static str, mode: Mode) -> bool {
        // A region's keys have no grammar: no counts, no operators, nothing to hold between
        // presses but the sequence itself.
        let mut keys = self.inner.region_keys.borrow_mut();
        keys.push(chord);
        // Which map a part-typed sequence belongs to, so which-key shows the region's rows rather
        // than the ones underneath them.
        self.inner.typing.set(Some(Typing { region, mode }));

        // What to do, decided while the maps are borrowed, and done after they are not: an action
        // can load a keymap.
        let outcome = self.inner.keymaps.with_layered(Some(region), |layered| {
            match layered.resolve(mode, &keys) {
                Resolution::Pending(_) => Region::Waiting,
                Resolution::None => Region::Unbound,
                Resolution::Run(binding) => Region::Run(binding.actions.clone()),
            }
        });

        if !matches!(outcome, Region::Waiting) {
            keys.clear();
            self.inner.typing.set(None);
        }
        drop(keys);
        self.publish_region();

        match outcome {
            Region::Waiting => true,
            Region::Unbound => false,
            Region::Run(actions) => {
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
        self.inner.keymaps.with_layered(None, |layered| {
            match layered.resolve(self.mode_untracked(), &[chord]) {
                Resolution::Run(binding) => binding.actions.iter().any(|one| one.name == action),
                _ => false,
            }
        })
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
