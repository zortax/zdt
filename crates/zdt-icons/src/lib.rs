//! The vendored icon set.
//!
//! Lucide art, compiled in as its own source. Each icon is a stroked 24-unit outline that names
//! `currentColor`, so the drawing takes the colour of the text beside it and one file serves every
//! state a control has. Size comes from the `.icon` class in the application's style sheet.
//! Nothing here decides how large an icon is.
//!
//! The whole document crosses to the paint stage, and not a path string. Lucide is stroked, and
//! its round caps and joins are part of how it reads at fourteen pixels.

use zgui::prelude::*;
use zgui::reactive::LocalStorage;
use zgui::{component, view};

/// Names the icons and reads their files.
macro_rules! icons {
    ($($name:ident => $file:literal,)*) => {
        $(
            #[doc = concat!("The `", $file, "` outline.")]
            pub const $name: &str = include_str!(
                concat!("../assets/", $file, ".svg")
            );
        )*

        /// Every icon, by file name. For a picker over the set, and for the tests.
        pub const ALL: &[(&str, &str)] = &[$(($file, $name)),*];
    };
}

icons! {
    ARCHIVE => "archive",
    ARROW_RIGHT => "arrow-right",
    BOOK_OPEN => "book-open",
    BOT => "bot",
    BRACES => "braces",
    BRAIN => "brain",
    CHECK => "check",
    CHEVRON_DOWN => "chevron-down",
    CHEVRON_LEFT => "chevron-left",
    CHEVRON_RIGHT => "chevron-right",
    CHEVRON_UP => "chevron-up",
    CIRCLE => "circle",
    CIRCLE_ALERT => "circle-alert",
    CIRCLE_DASHED => "circle-dashed",
    CIRCLE_CHECK => "circle-check",
    CIRCLE_QUESTION => "circle-question-mark",
    CIRCLE_X => "circle-x",
    CLIPBOARD_COPY => "clipboard-copy",
    CLOCK => "clock",
    CODE_XML => "code-xml",
    CLIPBOARD_PASTE => "clipboard-paste",
    COPY => "copy",
    CORNER_DOWN_LEFT => "corner-down-left",
    DOT => "dot",
    EXTERNAL_LINK => "external-link",
    EYE => "eye",
    EYE_OFF => "eye-off",
    FILE => "file",
    FILE_CODE => "file-code",
    FILE_DIFF => "file-diff",
    FILE_PLUS => "file-plus",
    FOLDER => "folder",
    FOLDER_GIT => "folder-git-2",
    FOLDER_OPEN => "folder-open",
    FOLDER_PLUS => "folder-plus",
    FOLDER_TREE => "folder-tree",
    FUNNEL => "funnel",
    GIT_BRANCH => "git-branch",
    GIT_BRANCH_PLUS => "git-branch-plus",
    GIT_COMMIT => "git-commit-horizontal",
    GLOBE => "globe",
    HASH => "hash",
    HISTORY => "history",
    INFO => "info",
    KEYBOARD => "keyboard",
    LANGUAGES => "languages",
    LIGHTBULB => "lightbulb",
    LIST_TODO => "list-todo",
    LIST_TREE => "list-tree",
    LOADER_CIRCLE => "loader-circle",
    MENU => "menu",
    MINUS => "minus",
    MOON => "moon",
    PALETTE => "palette",
    PANEL_BOTTOM => "panel-bottom",
    PANEL_LEFT => "panel-left",
    PENCIL => "pencil",
    PIN => "pin",
    PLUG => "plug",
    PLUS => "plus",
    REFRESH_CW => "refresh-cw",
    REPLACE => "replace",
    SAVE => "save",
    SCISSORS => "scissors",
    SEARCH => "search",
    SEND_HORIZONTAL => "send-horizontal",
    SETTINGS => "settings",
    SPARKLES => "sparkles",
    SQUARE => "square",
    SQUARE_CHECK => "square-check",
    TERMINAL => "terminal",
    TRASH => "trash-2",
    TRIANGLE_ALERT => "triangle-alert",
    TYPE => "type",
    WORKFLOW => "workflow",
    WRENCH => "wrench",
    X => "x",
}

/// The provider marks, beside the outline set.
///
/// Brand art is filled, and each mark keeps its own colour rule: the Claude mark carries its
/// brand orange in every theme, and the OpenAI mark follows the text beside it.
pub const CLAUDE: &str = include_str!("../assets/brand/claude.svg");

/// The OpenAI mark. See [`CLAUDE`].
pub const OPENAI: &str = include_str!("../assets/brand/openai.svg");

/// Every brand mark, by provider word. For whoever maps a provider to its mark, and for the
/// tests.
pub const BRANDS: &[(&str, &str)] = &[("claude", CLAUDE), ("codex", OPENAI)];

/// The mark for a provider word, when there is one.
#[must_use]
pub fn brand(provider: &str) -> Option<&'static str> {
    BRANDS
        .iter()
        .find(|(word, _)| *word == provider)
        .map(|(_, mark)| *mark)
}

/// Draws one icon.
///
/// The outline is a signal so that a disclosure chevron is one element with two values rather
/// than two branches that swap a node in and out.
///
/// A reader is told nothing unless `label` says otherwise: an icon beside a word repeats that
/// word, and a tree holding both is one a screen reader reads twice.
#[component]
pub fn Icon(
    /// Which outline to draw.
    #[prop(into)]
    icon: Signal<&'static str, LocalStorage>,
    /// What it is called, when it carries the meaning on its own.
    #[prop(into, optional)]
    label: Option<String>,
    /// Classes merged after the icon's own.
    #[prop(into, optional)]
    class: Classes,
    /// Anything else the caller forwarded.
    #[prop(attrs)]
    attrs: Attrs,
) -> impl IntoView {
    let semantics = Attrs::new().a11y_from(match label {
        Some(text) => A11yBinding::new(Role::Image).label(text),
        None => A11yBinding::new(Role::Image).hidden(true),
    });

    view! {
        vector(
            class = "icon",
            class = class,
            prop:svg = move || zgui::vocab::PropValue::from(icon.get()),
            {..semantics},
            {..attrs},
        )
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_icon_is_a_stroked_outline_that_follows_the_text() {
        for (name, source) in super::ALL {
            assert!(
                source.contains("viewBox=\"0 0 24 24\""),
                "{name} is not 24 units"
            );
            assert!(
                source.contains("stroke=\"currentColor\""),
                "{name} does not take the colour around it"
            );
            assert!(source.contains("fill=\"none\""), "{name} is filled");
        }
    }

    #[test]
    fn every_brand_mark_is_filled_art_with_a_view_box() {
        for (name, source) in super::BRANDS {
            assert!(source.contains("viewBox=\"0 0 "), "{name} has no view box");
            assert!(source.contains("fill="), "{name} names no fill");
            assert!(!source.contains("stroke="), "{name} is stroked");
        }
    }

    #[test]
    fn the_set_has_no_duplicate_names() {
        let mut seen: Vec<&str> = super::ALL.iter().map(|(name, _)| *name).collect();
        let count = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), count, "two constants read the same file");
    }
}
