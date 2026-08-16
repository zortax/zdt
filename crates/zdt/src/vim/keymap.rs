//! Loading keymaps, and layering overlays over them.

use super::*;

impl Vim {
    /// Puts the keymap back to the one the editor ships with.
    ///
    /// What a reload does before reading a person's file again: layering the new file onto what is
    /// already there would leave behind every row they have since deleted.
    pub fn reset_keymap(&self) {
        let mut keymap = Keymap::new();
        if let Err(problems) = merge(&mut keymap, DEFAULTS, Leaders::default()) {
            for problem in problems {
                tracing::error!("the shipped keymap: {problem}");
            }
        }
        *self.inner.keymap.borrow_mut() = keymap;
    }

    /// Reads more keymap text on top of what is there, which is what a user's file is.
    pub fn merge_keymap(&self, text: &str, leaders: Leaders) -> Result<(), Vec<String>> {
        let mut keymap = self.inner.keymap.borrow_mut();
        merge(&mut keymap, text, leaders)
            .map_err(|problems| problems.iter().map(ToString::to_string).collect())
    }

    /// Puts a region's own keys in front of the base map, under `name`.
    pub fn set_overlay(&self, name: &str, keymap: Keymap) {
        self.inner
            .overlays
            .borrow_mut()
            .insert(name.to_owned(), keymap);
    }

    /// Takes them off again.
    pub fn clear_overlay(&self, name: &str) {
        self.inner.overlays.borrow_mut().remove(name);
    }

    /// Reads a region's keymap out of text, and puts it in front under `name`.
    ///
    /// A region's keys are a file like every other keymap, so a person can change them: `extra` is
    /// read after `text`, which is where their own file goes.
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
        // Whatever did read is still installed: a region with most of its keys is more use than
        // one with none, and the problems are reported either way.
        self.set_overlay(name, keymap);
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }
}
