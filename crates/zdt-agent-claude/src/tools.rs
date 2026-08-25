//! What a tool call looks like to a person.
//!
//! The CLI names its tools and hands their input as JSON. This module turns both into the
//! vocabulary the timeline draws: a kind for the glyph, and one line saying what the call does.

use serde_json::Value;
use zdt_agent::thread::ToolKind;

/// What sort of work a tool name stands for.
#[must_use]
pub fn classify(name: &str) -> ToolKind {
    match name {
        "Read" | "Glob" | "NotebookRead" => ToolKind::Read,
        "Edit" | "Write" | "NotebookEdit" => ToolKind::Edit,
        "Bash" | "BashOutput" | "KillShell" => ToolKind::Execute,
        "Grep" => ToolKind::Search,
        "WebFetch" | "WebSearch" => ToolKind::Web,
        "TodoWrite" => ToolKind::Plan,
        _ if name.starts_with("mcp__") => ToolKind::Mcp,
        _ => ToolKind::Other,
    }
}

/// One line saying what the call does, from the field each tool is really about.
#[must_use]
pub fn summarize(name: &str, input: &Value) -> String {
    let field = |key: &str| {
        input
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|held| !held.is_empty())
    };
    let said = match name {
        "Bash" => field("command"),
        "Read" | "Edit" | "Write" => field("file_path"),
        "NotebookEdit" => field("notebook_path"),
        "Glob" | "Grep" => field("pattern"),
        "WebFetch" => field("url"),
        "WebSearch" => field("query"),
        "Task" | "Agent" => field("description").or_else(|| field("prompt")),
        _ => None,
    };
    let said = said.map(str::to_owned).unwrap_or_else(|| {
        let json = serde_json::to_string(input).unwrap_or_default();
        if json == "{}" || json == "null" {
            String::new()
        } else {
            json
        }
    });
    clip(&said, 200)
}

/// The whole input, laid out for reading before deciding.
#[must_use]
pub fn detail(input: &Value) -> String {
    match input {
        Value::Object(map) if map.len() == 1 => {
            // One field reads better bare: a command, a path, a pattern.
            match map.values().next() {
                Some(Value::String(text)) => clip(text, 16 * 1024),
                _ => clip(&pretty(input), 16 * 1024),
            }
        }
        _ => clip(&pretty(input), 16 * 1024),
    }
}

fn pretty(input: &Value) -> String {
    serde_json::to_string_pretty(input).unwrap_or_default()
}

/// At most `most` bytes, cut on a character edge, with an ellipsis when something was cut.
#[must_use]
pub fn clip(text: &str, most: usize) -> String {
    if text.len() <= most {
        return text.to_owned();
    }
    let mut end = most;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\u{2026}", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_is_the_summary_of_a_bash_call() {
        let input = serde_json::json!({"command": "cargo test", "timeout": 5});
        assert_eq!(summarize("Bash", &input), "cargo test");
    }

    #[test]
    fn an_unknown_tool_summarizes_as_its_input() {
        let input = serde_json::json!({"x": 1});
        assert_eq!(summarize("Strange", &input), "{\"x\":1}");
    }

    #[test]
    fn an_mcp_tool_is_classified_by_its_prefix() {
        assert_eq!(classify("mcp__linear__create_issue"), ToolKind::Mcp);
    }

    #[test]
    fn a_clip_lands_on_a_character_edge() {
        let text = "aé".repeat(100);
        let cut = clip(&text, 5);
        assert!(cut.ends_with('\u{2026}'));
    }
}
