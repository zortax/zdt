//! Conversations the CLI already has, read for import.
//!
//! Codex writes one JSONL rollout per thread under `CODEX_HOME/sessions/YYYY/MM/DD/`. The
//! `session_meta` line names the thread and its working directory — the resume cursor an
//! imported zdt thread starts with — and `response_item` message lines carry the prose.

use std::path::{Path, PathBuf};

use serde_json::Value;
use zdt_agent_harness::{DumpLine, FoundImport, SessionDump};

/// The rollouts the CLI holds under `home`, newest first.
///
/// `home` empty means the CLI's own default directory. Rollouts with no prompt in them are
/// left out.
#[must_use]
pub fn list(home: &str) -> Vec<FoundImport> {
    let mut found = Vec::new();
    for path in rollouts(home) {
        let Some(dump) = read_file(&path, false) else {
            continue;
        };
        let at_ms = std::fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|touched| touched.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |since| since.as_millis() as u64);
        found.push(FoundImport {
            id: dump.id,
            title: dump.title,
            cwd: dump.cwd,
            at_ms,
        });
    }
    found.sort_by_key(|session| std::cmp::Reverse(session.at_ms));
    found
}

/// One whole rollout under `home`, by its thread id.
#[must_use]
pub fn read(home: &str, id: &str) -> Option<SessionDump> {
    rollouts(home)
        .into_iter()
        .find(|path| {
            path.file_stem()
                .is_some_and(|stem| stem.to_string_lossy().ends_with(id))
        })
        .and_then(|path| read_file(&path, true))
        .filter(|dump| dump.id == id)
}

/// Every rollout file under `home`, in no particular order.
fn rollouts(home: &str) -> Vec<PathBuf> {
    let Some(sessions) = sessions_dir(home) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let mut stack = vec![sessions];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|kind| kind == "jsonl")
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("rollout-"))
            {
                found.push(path);
            }
        }
    }
    found
}

/// One rollout, parsed. `whole` keeps every message; a listing stops once it knows enough.
fn read_file(path: &Path, whole: bool) -> Option<SessionDump> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut id = String::new();
    let mut cwd = PathBuf::new();
    let mut lines: Vec<DumpLine> = Vec::new();
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match value["type"].as_str() {
            Some("session_meta") => {
                let meta = &value["payload"];
                id = meta["id"].as_str().unwrap_or_default().to_owned();
                cwd = PathBuf::from(meta["cwd"].as_str().unwrap_or_default());
            }
            Some("response_item") => {
                let item = &value["payload"];
                if item["type"].as_str() != Some("message") {
                    continue;
                }
                let user = match item["role"].as_str() {
                    Some("user") => true,
                    Some("assistant") => false,
                    _ => continue,
                };
                let text: String = item["content"]
                    .as_array()
                    .map(|blocks| {
                        blocks
                            .iter()
                            .filter_map(|block| block["text"].as_str())
                            .collect::<Vec<&str>>()
                            .join("\n")
                    })
                    .unwrap_or_default()
                    .trim()
                    .to_owned();
                // Instruction and environment wrappers are the harness talking to itself.
                if text.is_empty() || text.starts_with('<') {
                    continue;
                }
                lines.push(DumpLine { user, text });
                if !whole && lines.iter().any(|held| held.user) {
                    break;
                }
            }
            _ => {}
        }
    }
    if id.is_empty() || !lines.iter().any(|held| held.user) {
        return None;
    }
    let title = clip(
        &lines
            .iter()
            .find(|held| held.user)
            .map(|held| held.text.clone())
            .unwrap_or_default(),
    );
    Some(SessionDump {
        id,
        title,
        cwd,
        lines,
    })
}

/// The CLI's sessions directory: under `home` when one is named, its default otherwise.
fn sessions_dir(home: &str) -> Option<PathBuf> {
    if !home.is_empty() {
        return Some(PathBuf::from(home).join("sessions"));
    }
    std::env::home_dir().map(|dir| dir.join(".codex").join("sessions"))
}

/// One line of `text`, short enough for a sidebar row.
fn clip(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default().trim();
    let mut short: String = line.chars().take(60).collect();
    if short.len() < line.len() {
        short.push('\u{2026}');
    }
    short
}
