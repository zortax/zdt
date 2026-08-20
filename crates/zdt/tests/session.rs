//! What a session's two roots mean once the servers read them.
//!
//! The tree and the pickers see the directory that was opened. The servers see whatever encloses
//! it. These two crates only meet in this one, so the agreement between them is pinned here.

use std::path::Path;

use zdt_core::Project;

/// The workspace this test is compiled in, and the crate inside it.
fn crate_and_workspace() -> (&'static Path, std::path::PathBuf) {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = here
        .parent()
        .and_then(Path::parent)
        .expect("the crate is two levels under the workspace")
        .to_path_buf();
    (here, workspace)
}

#[test]
fn a_session_on_a_subdirectory_still_reaches_the_workspace_server() {
    let (here, workspace) = crate_and_workspace();
    let project = Project::session(here);

    // What somebody asked for.
    assert_eq!(project.root(), here);
    // And what rust-analyzer has to be told, or it indexes one crate of a workspace.
    assert_eq!(project.tooling_root(), workspace);

    let markers = vec!["Cargo.toml".to_owned()];
    let file = here.join("src").join("main.rs");
    let root = zdt_lsp::registry::root_for(&file, project.tooling_root(), &markers);
    assert_eq!(root.as_deref(), Some(workspace.as_path()));
}

#[test]
fn a_file_beside_the_session_is_still_inside_the_tooling_root() {
    // The whole reason the tooling root exists: `root_for` answers nothing for a file outside the
    // root it is given, so a session rooted at the crate would leave a sibling crate serverless.
    let (here, workspace) = crate_and_workspace();
    let project = Project::session(here);
    let sibling = workspace
        .join("crates")
        .join("zdt-core")
        .join("src")
        .join("lib.rs");

    let markers = vec!["Cargo.toml".to_owned()];
    assert_eq!(
        zdt_lsp::registry::root_for(&sibling, project.tooling_root(), &markers).as_deref(),
        Some(workspace.as_path()),
    );
    assert_eq!(
        zdt_lsp::registry::root_for(&sibling, project.root(), &markers),
        None,
        "the session's own root cannot serve a file beside it",
    );
}
