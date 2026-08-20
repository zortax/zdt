//! What every key is bound to.
//!
//! One map for the whole application: the shipped keymap, a person's own file on top of it, and
//! each region's own rows in front of both. Configuration, and therefore the same in every window
//! and every session — a keymap that differed between two windows of one editor would be a
//! keymap nobody could learn.
//!
//! The modal layer beside this holds what is *being typed*, which is the session's and not
//! shared. See [`crate::vim`].

use std::cell::RefCell;
use std::rc::Rc;

use zdt_vim::config::merge;
use zdt_vim::keymap::{Keymap, Layered};
use zdt_vim::notation::Leaders;
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

/// The keymap the editor ships with.
use crate::assets::KEYMAP as DEFAULTS;

/// Every binding there is.
///
/// Cloning one is cloning a handle: every clone reads the same maps.
#[derive(Clone)]
pub struct Keymaps {
    inner: Rc<Inner>,
}

struct Inner {
    /// The shipped map with a person's own read over it.
    base: RefCell<Keymap>,
    /// Each region's own keys, in front of the base map: the tree, a picker, a terminal.
    overlays: RefCell<rustc_hash::FxHashMap<String, Keymap>>,
    /// Counts up whenever anything here changes, so a panel drawn from it redraws.
    revision: RwSignal<u64, LocalStorage>,
}

impl Keymaps {
    /// The shipped keymap, and nothing else yet.
    ///
    /// A keymap that does not read is a bug in the editor, and not in anybody's configuration.
    /// It is reported, and the editor carries on with whatever did read.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Rc::new(Inner {
                base: RefCell::new(shipped()),
                overlays: RefCell::new(rustc_hash::FxHashMap::default()),
                revision: RwSignal::new_local(0),
            }),
        }
    }

    /// Puts the base map back to the one the editor ships with.
    ///
    /// What a reload does before reading a person's file again: layering the new file onto what is
    /// already there would leave behind every row they have since deleted.
    pub fn reset(&self) {
        *self.inner.base.borrow_mut() = shipped();
        self.changed();
    }

    /// Reads more keymap text on top of the base map, which is what a person's file is.
    ///
    /// # Errors
    ///
    /// The rows that did not read. Whatever did read is installed either way.
    pub fn merge(&self, text: &str, leaders: Leaders) -> Result<(), Vec<String>> {
        let outcome = {
            let mut base = self.inner.base.borrow_mut();
            merge(&mut base, text, leaders)
                .map_err(|problems| problems.iter().map(ToString::to_string).collect())
        };
        self.changed();
        outcome
    }

    /// Puts a region's own keys in front of the base map, under `name`.
    pub fn set_overlay(&self, name: &str, keymap: Keymap) {
        self.inner
            .overlays
            .borrow_mut()
            .insert(name.to_owned(), keymap);
        self.changed();
    }

    /// Takes them off again.
    pub fn clear_overlay(&self, name: &str) {
        self.inner.overlays.borrow_mut().remove(name);
        self.changed();
    }

    /// Reads a region's keymap out of text, and puts it in front under `name`.
    ///
    /// A region's keys are a file like every other keymap, so a person can change them: `extra` is
    /// read after `text`, which is where their own file goes.
    ///
    /// # Errors
    ///
    /// The rows that did not read. Whatever did read is installed either way, because a region
    /// with most of its keys is more use than one with none.
    pub fn load_overlay(
        &self,
        name: &str,
        text: &str,
        extra: Option<&str>,
    ) -> Result<(), Vec<String>> {
        let mut keymap = Keymap::new();
        let mut problems: Vec<String> = Vec::new();
        for source in std::iter::once(text).chain(extra) {
            if let Err(found) = merge(&mut keymap, source, Leaders::default()) {
                problems.extend(found.iter().map(ToString::to_string));
            }
        }
        self.set_overlay(name, keymap);
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }

    /// Reads the base map with `region`'s rows in front of it, when there is such a region.
    ///
    /// A closure, because both maps are behind their own `RefCell` and resolving needs the two of
    /// them at once. It also means the borrows are over before whatever was resolved is run,
    /// which matters: an action can load a keymap.
    pub fn with_layered<R>(&self, region: Option<&str>, read: impl FnOnce(&Layered<'_>) -> R) -> R {
        let overlays = self.inner.overlays.borrow();
        let base = self.inner.base.borrow();
        // A region with no keymap of its own still gets the base map: a terminal in normal mode
        // has no rows of its own, and `<Leader>ff` from inside one has to work.
        let layered = match region.and_then(|name| overlays.get(name)) {
            Some(map) => Layered::new(map, &base),
            None => Layered::plain(&base),
        };
        read(&layered)
    }

    /// Reads the base map on its own.
    pub fn with_base<R>(&self, read: impl FnOnce(&Keymap) -> R) -> R {
        read(&self.inner.base.borrow())
    }

    /// The keys that run `action` in `region`, in the notation a keymap is written in.
    ///
    /// Shortest first, so a menu showing one of them shows the single key rather than the
    /// sequence. Arguments are part of the question: `tree.create` with a directory flag is a
    /// different row from `tree.create` without one.
    ///
    /// A walk of every binding, so this is for a menu opening and never for a frame. Untracked; a
    /// caller that redraws when the keymap is read again reads [`revision`](Self::revision) beside
    /// it.
    #[must_use]
    pub fn keys_for(
        &self,
        region: Option<&str>,
        mode: zdt_vim::Mode,
        action: &zdt_vim::Action,
    ) -> Vec<String> {
        self.with_layered(region, |layered| {
            let mut found: Vec<Vec<zdt_vim::Chord>> = layered
                .bindings(mode)
                .into_iter()
                .filter(|(_, binding)| binding.actions.iter().any(|one| one == action))
                .map(|(keys, _)| keys)
                .collect();
            found.sort_by_key(Vec::len);
            found
                .iter()
                .map(|keys| zdt_vim::notation::format(keys))
                .collect()
        })
    }

    /// Changes whenever anything bound changes.
    ///
    /// Tracked. What a panel listing bindings reads so that a reload redraws it.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.inner.revision.get()
    }

    /// Says something bound changed.
    fn changed(&self) {
        self.inner.revision.update(|held| *held += 1);
    }
}

impl Default for Keymaps {
    fn default() -> Self {
        Self::new()
    }
}

/// The shipped keymap, read.
fn shipped() -> Keymap {
    let mut keymap = Keymap::new();
    if let Err(problems) = merge(&mut keymap, DEFAULTS, Leaders::default()) {
        for problem in problems {
            tracing::error!("the shipped keymap: {problem}");
        }
    }
    keymap
}

/// Publishes `keymaps` to every scope below this one.
pub fn provide(keymaps: Keymaps) {
    zgui::reactive::provide_local_context(keymaps);
}

/// The keymaps, from inside a component.
///
/// # Panics
///
/// If none was provided above this component. That is a wiring mistake, and nothing can carry on
/// from it.
#[must_use]
pub fn use_keymaps() -> Keymaps {
    zgui::reactive::use_local_context::<Keymaps>().expect("keymaps are provided at the root")
}

#[cfg(test)]
mod tests {
    use super::Keymaps;
    use zdt_vim::keymap::Resolution;
    use zdt_vim::{Chord, Mode};
    use zgui::prelude::*;

    fn chord(text: &str) -> Chord {
        zdt_vim::notation::parse(text, zdt_vim::notation::Leaders::default())
            .expect("the notation reads")[0]
    }

    /// A reactive runtime, so the revision signal can be made. Installing twice is nothing.
    fn ready() {
        zgui::reactive::install().expect("the reactive runtime installs");
    }

    #[test]
    fn the_shipped_map_is_there_from_the_start() {
        ready();
        let keymaps = Keymaps::new();
        let found = keymaps.with_layered(None, |layered| {
            matches!(
                layered.resolve(Mode::Normal, &[chord("i")]),
                Resolution::Run(_)
            )
        });
        assert!(found, "`i` is bound in the shipped keymap");
    }

    #[test]
    fn a_region_overlay_wins_over_the_base_map() {
        ready();
        let keymaps = Keymaps::new();
        keymaps
            .load_overlay(
                "tree",
                "[[map]]\nkeys = \"i\"\naction = \"tree.nothing\"\n",
                None,
            )
            .expect("the overlay reads");

        let named = |region| {
            keymaps.with_layered(region, |layered| {
                match layered.resolve(Mode::Normal, &[chord("i")]) {
                    Resolution::Run(binding) => binding.actions[0].name.clone(),
                    _ => String::new(),
                }
            })
        };
        assert_eq!(named(Some("tree")), "tree.nothing");
        assert_ne!(named(None), "tree.nothing");
    }

    #[test]
    fn resetting_forgets_what_was_read_over_the_shipped_map() {
        ready();
        let keymaps = Keymaps::new();
        let before = keymaps.with_base(|base| base.bindings(Mode::Normal).len());
        keymaps
            .merge(
                "[[map]]\nkeys = \"<Leader>zz\"\naction = \"app.quit\"\n",
                zdt_vim::notation::Leaders::default(),
            )
            .expect("the row reads");
        assert!(keymaps.with_base(|base| base.bindings(Mode::Normal).len()) > before);

        keymaps.reset();
        assert_eq!(
            keymaps.with_base(|base| base.bindings(Mode::Normal).len()),
            before
        );
    }

    #[test]
    fn every_change_moves_the_revision() {
        ready();
        let keymaps = Keymaps::new();
        let mut seen = keymaps.inner.revision.get_untracked();
        for step in [0, 1] {
            match step {
                0 => keymaps.set_overlay("picker", zdt_vim::keymap::Keymap::new()),
                _ => keymaps.clear_overlay("picker"),
            }
            let now = keymaps.inner.revision.get_untracked();
            assert!(now > seen, "a change moves the revision");
            seen = now;
        }
    }
}
