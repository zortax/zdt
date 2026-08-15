//! Which server runs where.
//!
//! Two questions, and neither involves talking to anything: which servers claim a file's language,
//! and which directory each of them should be rooted at.
//!
//! The root matters more than it looks. `rust-analyzer` rooted at a crate inside a workspace
//! indexes that crate alone and answers "no definition" for anything outside it; rooted at the
//! workspace it answers properly. So the rule is the outermost marker under the project, not the
//! nearest one — the same rule the project itself is found by.

use std::path::{Path, PathBuf};

use zdt_core::config::Server;

/// One server, ready to be started.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Wanted {
    /// The name the configuration knows it as, which is also how a client is keyed.
    pub name: String,
    /// What to run.
    pub command: String,
    /// Its arguments.
    pub args: Vec<String>,
    /// Where to run it.
    pub root: PathBuf,
    /// What to hand it as `initializationOptions`.
    pub initialization_options: Option<serde_json::Value>,
    /// What to answer `workspace/configuration` with.
    pub settings: Option<serde_json::Value>,
    /// What to add to its environment.
    pub env: std::collections::BTreeMap<String, String>,
}

/// Every server that claims `language`, rooted for `path`.
///
/// In configuration order, so a person who lists two servers for one language gets them in the
/// order they wrote. Nothing is started here.
#[must_use]
pub fn wanted_for(
    servers: &std::collections::BTreeMap<String, Server>,
    language: &str,
    path: &Path,
    project_root: &Path,
) -> Vec<Wanted> {
    servers
        .iter()
        .filter(|(_, server)| {
            server
                .filetypes
                .iter()
                .any(|claimed| claimed.eq_ignore_ascii_case(language))
        })
        .filter_map(|(name, server)| {
            let root = root_for(path, project_root, &server.root_markers)?;
            Some(Wanted {
                name: name.clone(),
                command: server.command.clone(),
                args: server.args.clone(),
                root,
                initialization_options: as_json(server.initialization_options.as_ref()),
                settings: as_json(server.settings.clone().as_ref()),
                env: server.env.clone(),
            })
        })
        .collect()
}

/// The configuration's TOML, as the JSON a server is handed.
///
/// The settings are written in TOML because the rest of the configuration is; the protocol speaks
/// JSON. A value that will not convert is left out rather than sent wrong.
fn as_json(value: Option<&toml::Value>) -> Option<serde_json::Value> {
    let value = value?;
    match serde_json::to_value(value) {
        Ok(json) => Some(json),
        Err(error) => {
            tracing::warn!("a server's settings are not expressible as JSON: {error}");
            None
        }
    }
}

/// Where a server for `path` should be rooted.
///
/// The outermost directory at or under `project_root` that holds one of `markers`; the project
/// root itself when none of them is found, because a server rooted somewhere is more use than no
/// server at all.
///
/// Answers nothing only when `path` is outside the project, which is a file opened from elsewhere
/// and not something a project's server should be told about.
#[must_use]
pub fn root_for(path: &Path, project_root: &Path, markers: &[String]) -> Option<PathBuf> {
    let directory = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };
    if !directory.starts_with(project_root) {
        return None;
    }
    if markers.is_empty() {
        return Some(project_root.to_path_buf());
    }

    // Climbing from the file to the project root and keeping the *last* match is what makes this
    // the outermost rather than the nearest.
    let mut outermost = None;
    let mut at = Some(directory.as_path());
    while let Some(here) = at {
        if markers.iter().any(|marker| here.join(marker).exists()) {
            outermost = Some(here.to_path_buf());
        }
        if here == project_root {
            break;
        }
        at = here.parent();
    }
    Some(outermost.unwrap_or_else(|| project_root.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    /// A workspace with a crate inside it.
    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("zdt-lsp-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("crates/inner/src")).expect("a directory");
            std::fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("a file");
            std::fs::write(root.join("crates/inner/Cargo.toml"), "[package]\n").expect("a file");
            std::fs::write(root.join("crates/inner/src/lib.rs"), "").expect("a file");
            Self(root)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn servers() -> BTreeMap<String, Server> {
        let mut servers = BTreeMap::new();
        servers.insert(
            "rust-analyzer".to_owned(),
            Server {
                command: "rust-analyzer".to_owned(),
                args: Vec::new(),
                filetypes: vec!["rust".to_owned()],
                root_markers: vec!["Cargo.toml".to_owned()],
                initialization_options: None,
                settings: None,
                env: BTreeMap::new(),
            },
        );
        servers.insert(
            "basedpyright".to_owned(),
            Server {
                command: "basedpyright-langserver".to_owned(),
                args: vec!["--stdio".to_owned()],
                filetypes: vec!["python".to_owned()],
                root_markers: vec!["pyproject.toml".to_owned()],
                initialization_options: None,
                settings: None,
                env: BTreeMap::new(),
            },
        );
        servers
    }

    #[test]
    fn the_outermost_marker_wins() {
        // Rooted at the inner crate, rust-analyzer would answer "no definition" for anything in
        // the rest of the workspace.
        let temp = Temp::new("outermost");
        let file = temp.0.join("crates/inner/src/lib.rs");
        assert_eq!(
            root_for(&file, &temp.0, &["Cargo.toml".to_owned()]),
            Some(temp.0.clone())
        );
    }

    #[test]
    fn no_marker_anywhere_is_the_project_itself() {
        let temp = Temp::new("nomarker");
        let file = temp.0.join("crates/inner/src/lib.rs");
        assert_eq!(
            root_for(&file, &temp.0, &["never-going-to-exist".to_owned()]),
            Some(temp.0.clone())
        );
    }

    #[test]
    fn a_file_outside_the_project_has_no_root() {
        let temp = Temp::new("outside");
        assert_eq!(
            root_for(
                Path::new("/somewhere/else/main.rs"),
                &temp.0,
                &["Cargo.toml".to_owned()]
            ),
            None
        );
    }

    #[test]
    fn only_the_servers_that_claim_the_language() {
        let temp = Temp::new("claims");
        let file = temp.0.join("crates/inner/src/lib.rs");

        let found = wanted_for(&servers(), "rust", &file, &temp.0);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "rust-analyzer");
        assert_eq!(found[0].root, temp.0);

        assert!(wanted_for(&servers(), "haskell", &file, &temp.0).is_empty());
    }

    #[test]
    fn a_language_is_matched_without_regard_to_case() {
        let temp = Temp::new("case");
        let file = temp.0.join("crates/inner/src/lib.rs");
        assert_eq!(wanted_for(&servers(), "Rust", &file, &temp.0).len(), 1);
    }

    #[test]
    fn the_arguments_come_through() {
        let temp = Temp::new("args");
        std::fs::write(temp.0.join("pyproject.toml"), "").expect("a file");
        let file = temp.0.join("crates/inner/src/lib.rs");

        let found = wanted_for(&servers(), "python", &file, &temp.0);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].command, "basedpyright-langserver");
        assert_eq!(found[0].args, vec!["--stdio".to_owned()]);
    }
}
