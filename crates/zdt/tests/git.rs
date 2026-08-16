//! The git panel, as this editor wires it.
//!
//! The panel itself is asserted in `zdt-gitui`, which knows nothing about a workspace. What is
//! asserted here is the seam: the overlay this editor ships reads, and the host it builds answers.

use zdt::workspace::Workspace;
use zdt_core::Project;
use zgui_testkit_view::Window;

fn keys() -> (Window, zdt::vim::Vim) {
    let window = Window::open();
    let vim = window.scope.with(|| {
        let workspace = Workspace::new(Project::at("/project"));
        let settings = zdt::settings::Settings::new(zdt_core::Config::default(), None);
        let vim = zdt::vim::Vim::new(workspace, settings);
        for (region, shipped, _) in zdt::assets::OVERLAYS {
            vim.load_overlay(region, shipped, None)
                .unwrap_or_else(|problems| panic!("{region} did not read: {problems:?}"));
        }
        vim
    });
    (window, vim)
}

#[test]
fn every_shipped_overlay_reads() {
    // The defect this prevents is silent: an overlay that does not parse is installed empty, and
    // the region it belongs to answers no keys at all. That looks exactly like a panel nobody
    // wired up.
    let (_window, _vim) = keys();
}

#[test]
fn the_git_region_is_one_of_them() {
    // The panel's keymap lives in `zdt-gitui` and reaches the window through `OVERLAYS`. A crate
    // that ships a keymap the editor never installs is a panel that answers nothing.
    assert!(
        zdt::assets::OVERLAYS
            .iter()
            .any(|(region, keymap, _)| *region == zdt_gitui::REGION && !keymap.is_empty()),
        "the git overlay is shipped"
    );
}

#[test]
fn the_panel_this_editor_builds_says_when_there_is_no_repository() {
    // The one thing the extraction can break silently. The panel is `zdt-gitui` and says nothing
    // on its own; whether the words reach the status line is entirely this editor's `Host`.
    let directory = std::env::temp_dir().join(format!("zdt-nogit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("a directory");

    let window = Window::open();
    let said = window.scope.with(|| {
        let workspace = Workspace::new(Project::at(&directory));
        // In the same order as `app.rs`. The host reads the vim layer and the announcements once,
        // when it is built, so both have to be in the scope by then.
        let settings = zdt::settings::Settings::new(zdt_core::Config::default(), None);
        zdt::workspace::provide(workspace.clone());
        zgui::reactive::provide_local_context(zdt::vim::Vim::new(workspace.clone(), settings));
        let panel = zdt::git::panel(workspace.clone());
        assert!(!panel.is_repository(), "the directory has no repository");
        panel.open();
        workspace.message().map(|message| message.text)
    });

    assert_eq!(
        said.as_deref(),
        Some("this project is not in a git repository")
    );
    let _ = std::fs::remove_dir_all(&directory);
}
