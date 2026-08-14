//! What every key sequence does, in every mode.
//!
//! A trie per mode. Walking it with what has been typed answers one of three things: nothing is
//! bound here, something is bound *below* here so keep waiting, or here is what to run. Which-key
//! is the second answer drawn.
//!
//! # Overlays
//!
//! A region with its own keys — the file tree, a picker, a terminal — puts an overlay in front of
//! the base map. The overlay is asked first and the base answers whatever it declines, so the tree
//! can bind `d` to *delete this file* without having to re-state what `<Leader>ff` does.

use rustc_hash::FxHashMap;

use crate::action::Action;
use crate::chord::Chord;
use crate::mode::{Mode, ModeSet};

/// One binding.
#[derive(Clone, PartialEq, Debug)]
pub struct Binding {
    /// What it does, in order.
    pub actions: Vec<Action>,
    /// What which-key calls it.
    pub description: String,
}

/// What walking the trie found.
#[derive(Clone, PartialEq, Debug)]
pub enum Resolution<'a> {
    /// Nothing is bound here and nothing is bound below here.
    None,
    /// Something is bound below here. These are the keys that would continue it.
    Pending(Vec<Continuation<'a>>),
    /// This is what to run.
    Run(&'a Binding),
}

/// One way a pending sequence could carry on.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Continuation<'a> {
    /// The key that would continue it.
    pub chord: Chord,
    /// What that key leads to.
    pub label: &'a str,
    /// Whether it is a whole binding rather than another prefix.
    pub runs: bool,
}

/// One node of one mode's trie.
#[derive(Default)]
struct Node {
    children: FxHashMap<Chord, Node>,
    binding: Option<Binding>,
    /// What which-key calls this prefix, when the keymap named it.
    group: Option<String>,
}

impl Node {
    /// The node at `keys`, when there is one.
    fn walk(&self, keys: &[Chord]) -> Option<&Node> {
        let mut node = self;
        for chord in keys {
            node = node.children.get(chord)?;
        }
        Some(node)
    }

    /// The node at `keys`, making it on the way.
    fn walk_mut(&mut self, keys: &[Chord]) -> &mut Node {
        let mut node = self;
        for chord in keys {
            node = node.children.entry(*chord).or_default();
        }
        node
    }

    /// Whether anything is bound here or under here.
    fn is_bare(&self) -> bool {
        self.binding.is_none() && self.children.is_empty()
    }
}

/// How many modes there are, which is how many tries a keymap holds.
const MODE_COUNT: usize = 10;

/// Which trie a mode is.
fn slot(mode: Mode) -> usize {
    mode as usize
}

/// Everything bound, in every mode.
#[derive(Default)]
pub struct Keymap {
    modes: Vec<Node>,
}

impl Keymap {
    /// A keymap with nothing in it.
    #[must_use]
    pub fn new() -> Self {
        Self {
            modes: (0..MODE_COUNT).map(|_| Node::default()).collect(),
        }
    }

    /// Binds `keys` to `binding` in every mode in `modes`.
    ///
    /// A later binding replaces an earlier one for the same keys in the same mode, which is what
    /// makes a user's file an override of the defaults rather than a conflict with them.
    pub fn bind(&mut self, modes: ModeSet, keys: &[Chord], binding: Binding) {
        if keys.is_empty() {
            return;
        }
        for mode in every_mode() {
            if modes.has(mode) {
                self.modes[slot(mode)].walk_mut(keys).binding = Some(binding.clone());
            }
        }
    }

    /// Unbinds `keys` in every mode in `modes`.
    ///
    /// The prefix above it stays if anything else hangs off it, so removing one leaf of a group
    /// leaves the rest of the group reachable.
    pub fn unbind(&mut self, modes: ModeSet, keys: &[Chord]) {
        for mode in every_mode() {
            if modes.has(mode) {
                remove(&mut self.modes[slot(mode)], keys);
            }
        }
    }

    /// Names a prefix, which is what which-key shows above the keys under it.
    pub fn name_group(&mut self, modes: ModeSet, keys: &[Chord], label: impl Into<String>) {
        if keys.is_empty() {
            return;
        }
        let label = label.into();
        for mode in every_mode() {
            if modes.has(mode) {
                self.modes[slot(mode)].walk_mut(keys).group = Some(label.clone());
            }
        }
    }

    /// What `keys` means in `mode`.
    #[must_use]
    pub fn resolve(&self, mode: Mode, keys: &[Chord]) -> Resolution<'_> {
        let Some(node) = self.modes[slot(mode)].walk(keys) else {
            return Resolution::None;
        };

        // A binding wins over a prefix: `d` deletes even though `dd` exists, and the engine
        // decides whether to wait by whether an operator is pending — which is a matter for the
        // grammar rather than for the map.
        if let Some(binding) = node.binding.as_ref() {
            return Resolution::Run(binding);
        }
        if node.children.is_empty() {
            return Resolution::None;
        }

        let mut continuations: Vec<Continuation<'_>> = node
            .children
            .iter()
            .map(|(chord, child)| Continuation {
                chord: *chord,
                label: child
                    .binding
                    .as_ref()
                    .map(|binding| binding.description.as_str())
                    .or(child.group.as_deref())
                    .unwrap_or(""),
                runs: child.binding.is_some(),
            })
            .collect();
        continuations.sort_by_key(|one| sort_key(one.chord));
        Resolution::Pending(continuations)
    }

    /// Whether anything at all is bound under `keys` in `mode`.
    #[must_use]
    pub fn has_prefix(&self, mode: Mode, keys: &[Chord]) -> bool {
        self.modes[slot(mode)]
            .walk(keys)
            .is_some_and(|node| !node.is_bare())
    }

    /// What the keymap calls the prefix `keys`, when it named it.
    #[must_use]
    pub fn group_label(&self, mode: Mode, keys: &[Chord]) -> Option<&str> {
        self.modes[slot(mode)].walk(keys)?.group.as_deref()
    }

    /// Every binding in `mode`, as key sequences and what they do.
    ///
    /// For the picker that searches the keymap, and for a test that wants to assert on the whole
    /// map rather than one row of it.
    #[must_use]
    pub fn bindings(&self, mode: Mode) -> Vec<(Vec<Chord>, &Binding)> {
        let mut found = Vec::new();
        collect(&self.modes[slot(mode)], &mut Vec::new(), &mut found);
        found
    }
}

/// One keymap in front of another.
///
/// The overlay answers first; the base answers whatever the overlay has nothing at all for. A key
/// the overlay knows as a prefix keeps waiting even when the base would have run something, which
/// is what makes a region's own two-key sequence possible.
pub struct Layered<'a> {
    /// The map in front, when there is one.
    pub overlay: Option<&'a Keymap>,
    /// The map behind.
    pub base: &'a Keymap,
}

impl<'a> Layered<'a> {
    /// Just the base.
    #[must_use]
    pub fn plain(base: &'a Keymap) -> Self {
        Self {
            overlay: None,
            base,
        }
    }

    /// An overlay in front of a base.
    #[must_use]
    pub fn new(overlay: &'a Keymap, base: &'a Keymap) -> Self {
        Self {
            overlay: Some(overlay),
            base,
        }
    }

    /// What `keys` means, asking the overlay first.
    #[must_use]
    pub fn resolve(&self, mode: Mode, keys: &[Chord]) -> Resolution<'a> {
        if let Some(overlay) = self.overlay {
            match overlay.resolve(mode, keys) {
                Resolution::None => {}
                answer => return answer,
            }
        }
        self.base.resolve(mode, keys)
    }
}

/// Every mode, for iterating the tries.
fn every_mode() -> [Mode; MODE_COUNT] {
    [
        Mode::Normal,
        Mode::Insert,
        Mode::Replace,
        Mode::Visual,
        Mode::VisualLine,
        Mode::VisualBlock,
        Mode::Select,
        Mode::OperatorPending,
        Mode::Command,
        Mode::Terminal,
    ]
}

/// Takes `keys` out of `node`, and takes out any prefix it leaves empty behind it.
fn remove(node: &mut Node, keys: &[Chord]) {
    let Some((first, rest)) = keys.split_first() else {
        node.binding = None;
        return;
    };
    let Some(child) = node.children.get_mut(first) else {
        return;
    };
    remove(child, rest);
    if child.is_bare() && child.group.is_none() {
        node.children.remove(first);
    }
}

/// Every binding under `node`, with the keys that reach it.
fn collect<'a>(node: &'a Node, keys: &mut Vec<Chord>, into: &mut Vec<(Vec<Chord>, &'a Binding)>) {
    if let Some(binding) = node.binding.as_ref() {
        into.push((keys.clone(), binding));
    }
    for (chord, child) in &node.children {
        keys.push(*chord);
        collect(child, keys, into);
        keys.pop();
    }
}

/// How the continuations of a prefix are ordered for a reader.
///
/// Characters first and in their own order, then everything with a modifier, then named keys —
/// so a which-key panel reads like an alphabet rather than like a hash map.
fn sort_key(chord: Chord) -> (u8, String, u8) {
    let modifiers = chord.mods.bits();
    match chord.key {
        crate::chord::Key::Char(character) => (0, character.to_string(), modifiers),
        // By name, so the named keys read alphabetically rather than in whatever order the enum
        // happens to be written in.
        crate::chord::Key::Named(named) => (1, named.as_str(), modifiers),
    }
}

#[cfg(test)]
mod tests {
    use super::{Binding, Keymap, Layered, Resolution};
    use crate::action::Action;
    use crate::chord::{Chord, Named};
    use crate::mode::{Mode, ModeSet};
    use crate::notation::{Leaders, parse};

    fn keys(sequence: &str) -> Vec<Chord> {
        parse(sequence, Leaders::default()).expect("the sequence reads")
    }

    fn binding(name: &str) -> Binding {
        Binding {
            actions: vec![Action::new(name)],
            description: name.to_owned(),
        }
    }

    fn map() -> Keymap {
        let mut map = Keymap::new();
        let normal = ModeSet::of(Mode::Normal);
        map.bind(normal, &keys("<Leader>ff"), binding("picker.files"));
        map.bind(normal, &keys("<Leader>fw"), binding("picker.grep"));
        map.bind(normal, &keys("<Leader>w"), binding("buffer.save"));
        map.bind(normal, &keys("gd"), binding("lsp.definition"));
        map.name_group(normal, &keys("<Leader>f"), "Find");
        map
    }

    #[test]
    fn a_whole_sequence_runs() {
        let map = map();
        match map.resolve(Mode::Normal, &keys("<Leader>ff")) {
            Resolution::Run(binding) => assert_eq!(binding.actions[0].name, "picker.files"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_prefix_waits_and_says_what_could_come_next() {
        let map = map();
        match map.resolve(Mode::Normal, &keys("<Leader>")) {
            Resolution::Pending(next) => {
                let chords: Vec<Chord> = next.iter().map(|one| one.chord).collect();
                assert!(chords.contains(&Chord::char('f')));
                assert!(chords.contains(&Chord::char('w')));
                let find = next
                    .iter()
                    .find(|one| one.chord == Chord::char('f'))
                    .expect("f is there");
                assert_eq!(find.label, "Find", "a named prefix shows its name");
                assert!(!find.runs, "it is a group rather than a binding");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn nothing_bound_is_nothing() {
        let map = map();
        assert_eq!(map.resolve(Mode::Normal, &keys("q")), Resolution::None);
        assert_eq!(
            map.resolve(Mode::Normal, &keys("<Leader>z")),
            Resolution::None
        );
    }

    #[test]
    fn a_binding_in_one_mode_is_not_in_another() {
        let map = map();
        assert_eq!(map.resolve(Mode::Insert, &keys("gd")), Resolution::None);
        assert!(matches!(
            map.resolve(Mode::Normal, &keys("gd")),
            Resolution::Run(_)
        ));
    }

    #[test]
    fn one_row_can_bind_several_modes() {
        let mut map = Keymap::new();
        let both = ModeSet::of(Mode::Normal).with(ModeSet::of(Mode::Visual));
        map.bind(both, &keys("<Leader>/"), binding("edit.comment"));
        for mode in [Mode::Normal, Mode::Visual] {
            assert!(matches!(
                map.resolve(mode, &keys("<Leader>/")),
                Resolution::Run(_)
            ));
        }
        assert_eq!(
            map.resolve(Mode::Insert, &keys("<Leader>/")),
            Resolution::None
        );
    }

    #[test]
    fn a_later_binding_replaces_an_earlier_one() {
        // Which is what makes a user's file an override rather than a conflict.
        let mut map = map();
        map.bind(
            ModeSet::of(Mode::Normal),
            &keys("<Leader>ff"),
            binding("picker.git_files"),
        );
        match map.resolve(Mode::Normal, &keys("<Leader>ff")) {
            Resolution::Run(binding) => assert_eq!(binding.actions[0].name, "picker.git_files"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn unbinding_a_leaf_leaves_the_rest_of_the_group() {
        let mut map = map();
        map.unbind(ModeSet::of(Mode::Normal), &keys("<Leader>ff"));
        assert_eq!(
            map.resolve(Mode::Normal, &keys("<Leader>ff")),
            Resolution::None
        );
        assert!(matches!(
            map.resolve(Mode::Normal, &keys("<Leader>fw")),
            Resolution::Run(_)
        ));
        assert!(map.has_prefix(Mode::Normal, &keys("<Leader>f")));
    }

    #[test]
    fn unbinding_the_last_leaf_takes_the_prefix_with_it() {
        let mut map = Keymap::new();
        let normal = ModeSet::of(Mode::Normal);
        map.bind(normal, &keys("<Leader>xq"), binding("list.quickfix"));
        map.unbind(normal, &keys("<Leader>xq"));
        assert!(
            !map.has_prefix(Mode::Normal, &keys("<Leader>x")),
            "a prefix with nothing under it would show an empty which-key panel"
        );
    }

    #[test]
    fn an_overlay_answers_before_the_base() {
        let base = map();
        let mut overlay = Keymap::new();
        overlay.bind(
            ModeSet::of(Mode::Normal),
            &keys("d"),
            binding("tree.delete"),
        );
        let layered = Layered::new(&overlay, &base);

        match layered.resolve(Mode::Normal, &keys("d")) {
            Resolution::Run(binding) => assert_eq!(binding.actions[0].name, "tree.delete"),
            other => panic!("{other:?}"),
        }
        // What the overlay says nothing about still works.
        assert!(matches!(
            layered.resolve(Mode::Normal, &keys("gd")),
            Resolution::Run(_)
        ));
    }

    #[test]
    fn an_overlay_prefix_beats_a_base_binding() {
        // The region's own two-key sequence has to be reachable even where the base would have
        // run something on the first key.
        let mut base = Keymap::new();
        base.bind(
            ModeSet::of(Mode::Normal),
            &keys("f"),
            binding("motion.find"),
        );
        let mut overlay = Keymap::new();
        overlay.bind(
            ModeSet::of(Mode::Normal),
            &keys("ff"),
            binding("tree.find_file"),
        );

        let layered = Layered::new(&overlay, &base);
        assert!(matches!(
            layered.resolve(Mode::Normal, &keys("f")),
            Resolution::Pending(_)
        ));
    }

    #[test]
    fn every_binding_can_be_listed() {
        let map = map();
        let all = map.bindings(Mode::Normal);
        assert_eq!(all.len(), 4);
        assert!(
            all.iter()
                .any(|(keys, binding)| keys.len() == 2
                    && binding.actions[0].name == "lsp.definition")
        );
    }

    #[test]
    fn continuations_read_in_an_order_a_person_can_scan() {
        let mut map = Keymap::new();
        let normal = ModeSet::of(Mode::Normal);
        for key in ["gz", "ga", "gm"] {
            map.bind(normal, &keys(key), binding(key));
        }
        map.bind(
            normal,
            &[Chord::char('g'), Chord::named(Named::Enter)],
            binding("g-enter"),
        );

        match map.resolve(Mode::Normal, &keys("g")) {
            Resolution::Pending(next) => {
                let chords: Vec<Chord> = next.iter().map(|one| one.chord).collect();
                assert_eq!(
                    chords,
                    vec![
                        Chord::char('a'),
                        Chord::char('m'),
                        Chord::char('z'),
                        Chord::named(Named::Enter),
                    ],
                    "characters in their own order, then the named keys"
                );
            }
            other => panic!("{other:?}"),
        }
    }
}
