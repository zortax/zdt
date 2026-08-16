//! What every one of the panel's named actions does.
//!
//! The keymap in `assets/keymap-git.toml` names these. One `match` is the whole registry, which is
//! the other half of that file: a key that resolves to `gitpanel.stage` arrives here.

use crate::panel::{GitUi, List, View};

/// Carries out `leaf`, the part of a `gitpanel.*` action after the dot.
///
/// Only reachable while the panel has the keyboard. Everything here acts on what the caret is on,
/// which the panel itself knows, so none of these takes an argument.
pub fn run(leaf: &str) {
    let Some(panel) = zgui::reactive::use_local_context::<GitUi>() else {
        return;
    };

    match leaf {
        "down" => panel.step(1),
        "up" => panel.step(-1),
        // Half a screenful, in rows. Every list has the same row height, and the key means "a
        // good way down".
        "half_down" => panel.step(10),
        "half_up" => panel.step(-10),
        "top" => panel.to_top(),
        "bottom" => panel.to_bottom(),
        "next_pane" => panel.cycle_list(true),
        "previous_pane" => panel.cycle_list(false),

        "toggle_view" => panel.toggle_view(),
        // These name a half. `1` and `2` are where you go, so pressing one twice is harmless.
        "status" => panel.show(View::Status),
        "history" => panel.show(View::History),
        "side_by_side" => panel.toggle_side_by_side(),

        "stage" => panel.stage(),
        "unstage" => panel.unstage(),
        "stage_all" => panel.stage_all(),
        "unstage_all" => panel.unstage_all(),
        "discard" => panel.discard(),

        "commit" => panel.start_commit(false),
        "amend" => panel.start_commit(true),

        "open" => panel.open_selected(),
        "checkout" => {
            panel.set_list(List::Branches);
            panel.checkout();
        }
        "refresh" => panel.refresh(),
        "to_tab" => panel.open_tab(),
        "close" => panel.close(),
        // Silently. The overlay is only in front while the panel is up, and an unbound key there
        // falls through.
        _ => {}
    }
}
