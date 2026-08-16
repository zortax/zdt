//! The keys the panel ships with.
//!
//! The keymap and the dispatcher are two halves of one contract: a row in `keymap-git.toml` names
//! an action, and `actions::run` answers it. A test over one without the other proves nothing.

/// The keymap the git region ships with, parsed the way a window parses it.
fn git_keymap() -> zdt_vim::Keymap {
    let mut keymap = zdt_vim::Keymap::new();
    zdt_vim::config::merge(&mut keymap, zdt_gitui::KEYMAP, zdt_vim::Leaders::default())
        .unwrap_or_else(|problems| panic!("keymap-git.toml did not read: {problems:?}"));
    keymap
}

/// What `keys` runs in the git region, when it runs anything.
fn bound(keymap: &zdt_vim::Keymap, keys: &[zdt_vim::chord::Chord]) -> Option<String> {
    match keymap.resolve(zdt_vim::Mode::Normal, keys) {
        zdt_vim::keymap::Resolution::Run(binding) => Some(binding.actions.first()?.name.clone()),
        _ => None,
    }
}

#[test]
fn the_panel_answers_the_keys_it_is_bound_to() {
    // Asserted against the *action*, not against whether the key was used: a region falls back to
    // the base map, so an unbound `s` in the panel would run the editor's substitute and still
    // report itself as used. Which is exactly how the panel shipped answering nothing.
    use zdt_vim::chord::Chord;

    let keymap = git_keymap();
    for (key, action) in [
        ('j', "gitpanel.down"),
        ('k', "gitpanel.up"),
        ('s', "gitpanel.stage"),
        ('u', "gitpanel.unstage"),
        ('c', "gitpanel.commit"),
        ('q', "gitpanel.close"),
        ('o', "gitpanel.open"),
        ('r', "gitpanel.refresh"),
        ('b', "gitpanel.checkout"),
        ('a', "gitpanel.stage_all"),
    ] {
        assert_eq!(
            bound(&keymap, &[Chord::char(key)]).as_deref(),
            Some(action),
            "`{key}` should run {action}"
        );
    }
}

#[test]
fn escape_closes_the_panel() {
    // The one that mattered: with no key handler on the panel, `<Esc>` went to the editor behind
    // it and the panel could only be dismissed by quitting the editor.
    use zdt_vim::chord::{Chord, Key, Mods, Named};

    let escape = Chord::new(Key::Named(Named::Escape), Mods::NONE);
    assert_eq!(
        bound(&git_keymap(), &[escape]).as_deref(),
        Some("gitpanel.close")
    );
}
