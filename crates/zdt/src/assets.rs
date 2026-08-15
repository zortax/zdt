//! What is compiled into the binary.
//!
//! The style sheets are joined into one string because a window takes exactly one sheet at
//! start-up; everything installed later goes through `install_stylesheet` from a view, which is
//! also how the theme and the user's own sheet reach the document.

/// The style sheets, in cascade order.
///
/// Later files win at equal specificity, so the order here is the order of increasing
/// specialisation: measurements first, then the frame, then each region.
const SHEETS: &[&str] = &[
    include_str!("../../../assets/css/base.css"),
    include_str!("../../../assets/css/frame.css"),
    include_str!("../../../assets/css/chrome.css"),
    include_str!("../../../assets/css/panes.css"),
    include_str!("../../../assets/css/tree.css"),
    include_str!("../../../assets/css/prompt.css"),
    include_str!("../../../assets/css/picker.css"),
    include_str!("../../../assets/css/terminal.css"),
    // Last, so that what moves is decided in one place rather than beside each thing that moves.
    include_str!("../../../assets/css/motion.css"),
    include_str!("../../../assets/css/whichkey.css"),
];

/// The keymap the editor ships with.
pub const KEYMAP: &str = include_str!("../../../assets/keymap.toml");

/// The file tree's own keys, read in front of the base map while the keyboard is in the panel.
pub const TREE_KEYMAP: &str = include_str!("../../../assets/keymap-tree.toml");

/// Every compiled-in style sheet, joined.
#[must_use]
pub fn sheet() -> String {
    SHEETS.join("\n")
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_sheet_carries_every_file() {
        let sheet = super::sheet();
        assert!(sheet.contains("--chrome:"), "base.css is missing");
        assert!(sheet.contains(".frame {"), "frame.css is missing");
    }

    #[test]
    fn nothing_in_the_sheet_uses_a_line_comment() {
        // `//` is not a comment in CSS, and a sheet carrying one loses the rule it is in with
        // nothing but a warning to say so.
        for (number, line) in super::sheet().lines().enumerate() {
            let trimmed = line.trim_start();
            assert!(
                !trimmed.starts_with("//"),
                "line {} is a `//` comment: {trimmed}",
                number + 1
            );
        }
    }
}
