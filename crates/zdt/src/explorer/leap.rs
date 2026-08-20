//! Leaping to a row by its name.
//!
//! The same three keystrokes the editor's leap is: `s`, two characters of a name, then the label
//! that appears on every row holding them.
//!
//! The finding and the labelling are [`zdt_vim::leap`]'s and are pure. The rows offered are the
//! ones on screen, which is what keeps the labels few and the choice honest.

use zdt_vim::Chord;
use zdt_vim::leap::{Direction, Landing};

use crate::explorer::Explorer;
use crate::leap::{Over, Took};
use crate::vim::Vim;

/// Takes one key for a leap over the tree. Answers whether the leap wanted it.
///
/// Every key belongs to a leap while one is running, the one that ends it included. A keymap that
/// answered one would put some letters out of reach.
pub fn key(vim: &Vim, explorer: &Explorer, chord: Chord) -> bool {
    use zdt_vim::chord::Key;

    let leaping = vim.leaping();
    if !leaping.is_running_over(Over::Tree) {
        return false;
    }

    // Only a plain character narrows or chooses. A chord with a modifier on it ends the leap, and
    // so does `<Esc>`, which is how anybody expects to get out.
    let character = match chord.key {
        Key::Char(character) if chord.mods.is_empty() => Some(character),
        _ => None,
    };

    let took = leaping.key(character, |pair, direction, alphabet| {
        landings(explorer, pair, direction, alphabet)
    });

    if let Took::Landed(at) = took {
        explorer.go_to(at);
    }
    true
}

/// Every visible row whose name holds `pair`, labelled.
fn landings(explorer: &Explorer, pair: &str, direction: Direction, alphabet: &str) -> Vec<Landing> {
    let Some(viewport) = explorer.viewport() else {
        return Vec::new();
    };
    let rows = explorer.rows();
    let seen = viewport.visible();

    let names: Vec<(usize, &str)> = (seen.start..seen.end.min(rows.len()))
        .filter_map(|at| Some((at, rows.get(at)?.entry.name.as_str())))
        .collect();

    zdt_vim::leap::list_landings(&names, explorer.at_untracked(), pair, direction, alphabet)
}
