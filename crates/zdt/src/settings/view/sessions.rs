//! Where the sessionizer looks for projects.

use super::*;
use zgui::prelude::*;
use zgui::reactive::{RenderEffect, RwSignal};
use zgui::{component, view};
use zgui_ui::prelude::*;

/// The directories a block of text names, one per line.
///
/// Blank lines are dropped and the space somebody leaves while typing is not part of a path.
fn directories(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// What `<Leader>Sf` offers without being asked.
#[component]
pub(crate) fn Sessions() -> impl IntoView {
    let settings = crate::settings::use_settings();

    // The field holds the text; the settings hold the list. They are deliberately not the same
    // value: a list joined back into text loses the empty line somebody has just opened to type
    // the next path on, so a field rendered from the list would delete every newline as it was
    // typed. So the text is written here, the list is derived from it, and the text is only ever
    // replaced when the *list* changes underneath — which is the file changing on disk.
    let text =
        RwSignal::new_local(settings.with_untracked(|config| config.sessions.paths.join("\n")));
    let following = {
        let settings = settings.clone();
        RenderEffect::new(move |_| {
            let held = settings.with(|config| config.sessions.paths.clone());
            if directories(&text.get_untracked()) != held {
                text.set(held.join("\n"));
            }
        })
    };
    on_cleanup_local(move || drop(following));

    let paths = {
        let settings = settings.clone();
        Binding::controlled(text, move |typed: String| {
            settings.edit(|config| config.sessions.paths = directories(&typed));
            text.set(typed);
        })
    };
    let depth = number(
        &settings,
        |config| config.sessions.depth as u32,
        |config, value| config.sessions.depth = value as usize,
    );
    let hidden = bound(
        &settings,
        |config| config.sessions.hidden,
        |config, value| config.sessions.hidden = value,
    );

    view! {
        SettingsGroup {
            SettingsGroupLabel {"Where projects are"}
            SettingsItem(
                label = "Directories to look in",
                description = "One per line. `~` is the home directory, and a directory that is \
                               not there is skipped."
            ) {
                Textarea(
                    class = "config__paths",
                    value = paths,
                    placeholder = "~/Projects",
                    {..use_settings_item_attrs()}
                )
            }
            SettingsItem(
                label = "How far down to look",
                description = "One is the directories in each. Two also takes their children, \
                               which is what a directory of owners each holding repositories \
                               wants. A project is offered but never entered, so this is never \
                               spent on the inside of one."
            ) {
                Number(value = depth, min = 1.0, max = 4.0, step = 1.0, unit = "")
            }
            SettingsItem(label = "Offer directories beginning with a dot") {
                Switch(class = "config__switch", checked = hidden, {..use_settings_item_attrs()})
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::directories;

    #[test]
    fn one_directory_per_line() {
        assert_eq!(
            directories("~/Projects\n~/work"),
            vec!["~/Projects".to_owned(), "~/work".to_owned()],
        );
    }

    #[test]
    fn a_line_being_typed_on_is_not_a_directory_yet() {
        // The case the field is built around: an empty last line is somebody part-way through
        // adding one, and must not become an entry.
        assert_eq!(directories("~/Projects\n"), vec!["~/Projects".to_owned()]);
        assert_eq!(
            directories("~/Projects\n\n  \n"),
            vec!["~/Projects".to_owned()]
        );
    }

    #[test]
    fn nothing_at_all_is_no_directories() {
        assert!(directories("").is_empty());
        assert!(directories("\n \n").is_empty());
    }

    #[test]
    fn the_space_around_a_path_is_not_part_of_it() {
        assert_eq!(directories("  ~/Projects  "), vec!["~/Projects".to_owned()]);
    }
}
