//! The skills on disk.
//!
//! The CLI reads skills from `.claude/skills` beside the project and under the user's own
//! configuration directory; each skill is a directory holding a `SKILL.md`. The init message
//! names them too, but only once a real turn runs — this scan answers before that.

use std::path::{Path, PathBuf};

/// Every skill name a session in `cwd` would see, sorted and deduplicated.
#[must_use]
pub fn discover(cwd: &Path) -> Vec<String> {
    let mut names = Vec::new();
    for base in places(cwd) {
        let Ok(entries) = std::fs::read_dir(base.join("skills")) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.join("SKILL.md").is_file()
                && let Some(name) = path.file_name().and_then(|name| name.to_str())
            {
                names.push(name.to_owned());
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

/// The `.claude` directories a session reads, nearest first.
fn places(cwd: &Path) -> Vec<PathBuf> {
    let mut places = Vec::new();
    places.push(cwd.join(".claude"));
    if let Ok(config) = std::env::var("CLAUDE_CONFIG_DIR") {
        if !config.is_empty() {
            places.push(PathBuf::from(config));
        }
    } else if let Some(home) = std::env::home_dir() {
        places.push(home.join(".claude"));
    }
    places
}
