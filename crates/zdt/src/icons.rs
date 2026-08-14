//! The vendored icon set.
//!
//! Lucide art, compiled in as its own source. Each icon is a stroked 24-unit outline that names
//! `currentColor`, so the drawing takes the colour of the text it sits beside and one file serves
//! every state a control has. Size comes from the `.icon` class in `assets/css/base.css`; nothing
//! here decides how large an icon is.
//!
//! The whole document crosses to the paint stage rather than a path string, because Lucide is
//! stroked and its round caps and joins are part of how it reads at fourteen pixels.

use zgui::prelude::*;
use zgui::reactive::LocalStorage;
use zgui::{component, view};

/// Names the icons and reads their files.
macro_rules! icons {
    ($($name:ident => $file:literal,)*) => {
        $(
            #[doc = concat!("The `", $file, "` outline.")]
            pub const $name: &str = include_str!(
                concat!("../../../assets/icons/lucide/", $file, ".svg")
            );
        )*

        /// Every icon, by file name. For a picker over the set, and for the tests.
        pub const ALL: &[(&str, &str)] = &[$(($file, $name)),*];
    };
}

icons! {
    ARROW_RIGHT => "arrow-right",
    BOOK_OPEN => "book-open",
    BRACES => "braces",
    CHEVRON_DOWN => "chevron-down",
    CHEVRON_LEFT => "chevron-left",
    CHEVRON_RIGHT => "chevron-right",
    CHEVRON_UP => "chevron-up",
    CIRCLE => "circle",
    CIRCLE_ALERT => "circle-alert",
    CIRCLE_CHECK => "circle-check",
    CIRCLE_X => "circle-x",
    COPY => "copy",
    CORNER_DOWN_LEFT => "corner-down-left",
    DOT => "dot",
    EYE => "eye",
    EYE_OFF => "eye-off",
    FILE => "file",
    FILE_CODE => "file-code",
    FOLDER => "folder",
    FOLDER_OPEN => "folder-open",
    FUNNEL => "funnel",
    GIT_BRANCH => "git-branch",
    HASH => "hash",
    INFO => "info",
    LIGHTBULB => "lightbulb",
    LIST_TREE => "list-tree",
    MENU => "menu",
    MINUS => "minus",
    PANEL_BOTTOM => "panel-bottom",
    PANEL_LEFT => "panel-left",
    PENCIL => "pencil",
    PLUS => "plus",
    REFRESH_CW => "refresh-cw",
    REPLACE => "replace",
    SAVE => "save",
    SEARCH => "search",
    SETTINGS => "settings",
    SQUARE => "square",
    TERMINAL => "terminal",
    TRASH => "trash-2",
    TRIANGLE_ALERT => "triangle-alert",
    TYPE => "type",
    WRENCH => "wrench",
    X => "x",
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
    fn the_set_has_no_duplicate_names() {
        let mut seen: Vec<&str> = super::ALL.iter().map(|(name, _)| *name).collect();
        let count = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), count, "two constants read the same file");
    }
}
