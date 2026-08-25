//! Conversations the CLI already has, read for import.
//!
//! The CLI writes one JSONL transcript per session under `<config dir>/projects/<escaped
//! path>/<session id>.jsonl`. The file name is the resume cursor, the lines carry the working
//! directory and the prose, and an `ai-title` line carries the CLI's own name for the
//! conversation. Reading them is enough to make a zdt thread that resumes where the CLI left
//! off.

use std::path::{Path, PathBuf};

use serde_json::Value;
use zdt_agent_harness::{DumpLine, FoundImport, SessionDump};

/// How much of a transcript the listing reads per file. Enough for the directory, the title
/// and the first prompt; a full read waits until a session is actually imported.
const SKIM: u64 = 262_144;

/// The sessions the CLI holds under `home`, newest first.
///
/// `home` empty means the CLI's own default directory. Sessions with no prompt in them — a
/// probe, an empty window — are left out.
#[must_use]
pub fn list(home: &str) -> Vec<FoundImport> {
    let Some(projects) = config_dir(home).map(|dir| dir.join("projects")) else {
        return Vec::new();
    };
    let Ok(groups) = std::fs::read_dir(&projects) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for group in groups.flatten() {
        let Ok(files) = std::fs::read_dir(group.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().is_none_or(|kind| kind != "jsonl") {
                continue;
            }
            let Some(id) = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
            else {
                continue;
            };
            let Some(read) = skim(&path) else {
                continue;
            };
            let at_ms = file
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|touched| touched.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |since| since.as_millis() as u64);
            found.push(FoundImport {
                id,
                title: read.title,
                cwd: read.cwd,
                at_ms,
            });
        }
    }
    found.sort_by_key(|session| std::cmp::Reverse(session.at_ms));
    found
}

/// One whole session under `home`, by its id.
#[must_use]
pub fn read(home: &str, id: &str) -> Option<SessionDump> {
    let projects = config_dir(home)?.join("projects");
    let groups = std::fs::read_dir(&projects).ok()?;
    for group in groups.flatten() {
        let path = group.path().join(format!("{id}.jsonl"));
        if !path.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&path).ok()?;
        let mut cwd = PathBuf::new();
        let mut title = String::new();
        let mut lines = Vec::new();
        for line in text.lines() {
            take_line(line, &mut cwd, &mut title, Some(&mut lines));
        }
        if lines.is_empty() {
            return None;
        }
        if title.is_empty() {
            title = clip(&lines[0].text);
        }
        return Some(SessionDump {
            id: id.to_owned(),
            title,
            cwd,
            lines,
        });
    }
    None
}

/// What a light read of one transcript answers.
struct Skimmed {
    cwd: PathBuf,
    title: String,
}

/// Reads the head of a transcript: the directory, a title, and whether anyone spoke.
fn skim(path: &Path) -> Option<Skimmed> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut text = String::new();
    let _ = std::io::Read::by_ref(&mut file)
        .take(SKIM)
        .read_to_string(&mut text);
    let mut cwd = PathBuf::new();
    let mut title = String::new();
    let mut first_prompt = String::new();
    let mut spoke = false;
    // The last line of a clipped read may be cut; lines() simply fails to parse it.
    for line in text.lines() {
        let mut lines: Vec<DumpLine> = Vec::new();
        take_line(line, &mut cwd, &mut title, Some(&mut lines));
        if let Some(said) = lines.into_iter().find(|held| held.user) {
            if first_prompt.is_empty() {
                first_prompt = said.text;
            }
            spoke = true;
        }
    }
    if !spoke {
        return None;
    }
    if title.is_empty() {
        title = clip(&first_prompt);
    }
    Some(Skimmed { cwd, title })
}

/// Folds one transcript line into the pieces an import needs.
fn take_line(line: &str, cwd: &mut PathBuf, title: &mut String, lines: Option<&mut Vec<DumpLine>>) {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return;
    };
    // Subagent narration stays out, as it does in the live timeline.
    if value["isSidechain"].as_bool() == Some(true) {
        return;
    }
    match value["type"].as_str() {
        // The CLI's own name for the conversation beats a clipped prompt.
        Some("ai-title") => {
            if let Some(said) = value["aiTitle"].as_str()
                && !said.is_empty()
            {
                *title = said.to_owned();
            }
        }
        Some("user") => {
            if cwd.as_os_str().is_empty()
                && let Some(spot) = value["cwd"].as_str()
            {
                *cwd = PathBuf::from(spot);
            }
            let text = message_text(&value["message"]);
            if !text.is_empty()
                && !text.starts_with('<')
                && let Some(lines) = lines
            {
                lines.push(DumpLine { user: true, text });
            }
        }
        Some("assistant") => {
            let text = message_text(&value["message"]);
            if !text.is_empty()
                && let Some(lines) = lines
            {
                lines.push(DumpLine { user: false, text });
            }
        }
        _ => {}
    }
}

/// The prose of one message: a plain string, or its text blocks joined.
fn message_text(message: &Value) -> String {
    match &message["content"] {
        Value::String(text) => text.trim().to_owned(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|block| block["type"].as_str() == Some("text"))
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<&str>>()
            .join("\n")
            .trim()
            .to_owned(),
        _ => String::new(),
    }
}

/// The CLI's configuration directory: `home` when one is named, its default otherwise.
fn config_dir(home: &str) -> Option<PathBuf> {
    if !home.is_empty() {
        return Some(PathBuf::from(home));
    }
    std::env::home_dir().map(|dir| dir.join(".claude"))
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
