//! The outlines a work row wears, and the words a span of time reads as.

use zdt_agent::thread::{ItemKind, ToolKind};
use zdt_icons as icons;

/// The outline for a tool of `tool`'s sort, for whoever draws one outside the timeline.
#[must_use]
pub fn tool_glyph_for(tool: ToolKind) -> &'static str {
    tool_glyph(ItemKind::Tool, tool)
}

/// The outline a healthy tool row carries.
pub(super) fn tool_glyph(kind: ItemKind, tool: ToolKind) -> &'static str {
    if kind == ItemKind::Task {
        return icons::BOT;
    }
    match tool {
        ToolKind::Read => icons::EYE,
        ToolKind::Edit => icons::PENCIL,
        ToolKind::Execute => icons::TERMINAL,
        ToolKind::Search => icons::SEARCH,
        ToolKind::Web => icons::GLOBE,
        ToolKind::Plan => icons::LIST_TODO,
        ToolKind::Mcp => icons::PLUG,
        ToolKind::Other => icons::WRENCH,
    }
}

/// A span of time in a few characters: "12s", "3m14s", "1h2m".
pub(super) fn span_text(ms: u64) -> String {
    let seconds = ms / 1000;
    if seconds >= 3600 {
        format!("{}h{}m", seconds / 3600, (seconds % 3600) / 60)
    } else if seconds >= 60 {
        format!("{}m{}s", seconds / 60, seconds % 60)
    } else {
        format!("{seconds}s")
    }
}
