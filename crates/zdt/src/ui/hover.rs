//! What the server says about the thing under the caret.
//!
//! A panel anchored to the caret, which is what `point_for_byte` was added for. It takes no
//! keyboard: `K` opens it, the next key closes it, and everything in between goes where it would
//! have gone anyway — because a documentation panel that swallows keys is one that has to be
//! dismissed before work can continue.
//!
//! The text is markdown. It is not rendered as markdown: what a hover holds is a signature, a
//! sentence and sometimes a code block, and the fences and the emphasis marks are noise once the
//! block is in a monospace panel. They are stripped; everything else is shown as written.

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::{component, view};

/// What is being shown, and where.
#[derive(Clone, PartialEq, Debug)]
pub struct Showing {
    /// The documentation, already stripped of its fences.
    pub text: String,
    /// Where the caret was, in window pixels.
    pub x: f32,
    /// The same.
    pub y: f32,
    /// How tall the caret's line is, so the panel can sit under it rather than over it.
    pub height: f32,
}

/// The one hover panel.
#[derive(Clone, Copy)]
pub struct Hover {
    showing: RwSignal<Option<Showing>, LocalStorage>,
}

impl Hover {
    /// Nothing showing.
    #[must_use]
    pub fn new() -> Self {
        Self {
            showing: RwSignal::new_local(None),
        }
    }

    /// Shows `text` at the caret.
    pub fn show(&self, text: &str, at: zgui_editor::CaretRect) {
        let text = tidy(text);
        if text.is_empty() {
            return;
        }
        self.showing.set(Some(Showing {
            text,
            x: at.x,
            y: at.y,
            height: at.height,
        }));
    }

    /// Puts it away.
    pub fn hide(&self) {
        if self.showing.with_untracked(Option::is_some) {
            self.showing.set(None);
        }
    }

    /// Whether anything is showing, without subscribing.
    #[must_use]
    pub fn is_showing(&self) -> bool {
        self.showing.with_untracked(Option::is_some)
    }

    /// What is showing. Tracked.
    #[must_use]
    pub fn showing(&self) -> Option<Showing> {
        self.showing.get()
    }
}

impl Default for Hover {
    fn default() -> Self {
        Self::new()
    }
}

/// Markdown as a monospace panel wants it.
///
/// Fences and heading marks come out; everything else stays as written, including the blank lines
/// — a signature and its description separated by one is how every server writes a hover, and
/// closing that gap makes them one paragraph.
#[must_use]
pub fn tidy(text: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim_start().starts_with("```") {
            continue;
        }
        // `---` between the signature and the prose is a rule nothing draws here.
        if trimmed.trim() == "---" || trimmed.trim() == "___" {
            continue;
        }
        lines.push(trimmed.trim_start_matches('#').trim_start().to_owned());
    }

    // Runs of blank lines become one, and the ends are trimmed: a hover that opens on three empty
    // rows is a panel mostly made of nothing.
    let mut tidied: Vec<String> = Vec::new();
    for line in lines {
        if line.is_empty() && tidied.last().is_some_and(String::is_empty) {
            continue;
        }
        tidied.push(line);
    }
    while tidied.first().is_some_and(String::is_empty) {
        tidied.remove(0);
    }
    while tidied.last().is_some_and(String::is_empty) {
        tidied.pop();
    }
    tidied.join("\n")
}

/// Puts the hover where every component can find it.
pub fn provide(hover: Hover) {
    zgui::reactive::provide_local_context(hover);
}

/// It, from inside a component.
///
/// # Panics
///
/// If none was provided above this component, which is a wiring mistake.
#[must_use]
pub fn use_hover() -> Hover {
    zgui::reactive::use_local_context::<Hover>().expect("a hover is provided at the root")
}

/// The panel.
#[component]
pub fn HoverPanel() -> impl IntoView {
    let hover = use_hover();

    view! {
        {move || {
            use crate::ui::Erase;
            match hover.showing() {
                Some(showing) => view! { Panel(showing = showing) }.any(),
                None => ().any(),
            }
        }}
    }
}

/// One panel of documentation.
#[component]
fn Panel(
    /// What to show, and where.
    showing: Showing,
) -> impl IntoView {
    // Under the caret's line rather than over it: what a person pressed `K` about is the thing the
    // caret is on, and covering it to describe it is not a help.
    let top = format!("{}px", showing.y + showing.height + 2.0);
    let left = format!("{}px", showing.x);
    let lines: Vec<String> = showing.text.lines().map(str::to_owned).collect();

    view! {
        column(
            class = "hover",
            style:left = Some(left),
            style:top = Some(top),
            a11y:role = Role::Tooltip,
            a11y:label = "Documentation"
        ) {
            {lines
                .into_iter()
                .map(|line| view! { label(class = "hover__line") {{line}} })
                .collect::<Vec<_>>()}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tidy;

    #[test]
    fn fences_come_out_and_the_code_stays() {
        let text = "```rust\nfn main()\n```\nDoes the thing.";
        assert_eq!(tidy(text), "fn main()\nDoes the thing.");
    }

    #[test]
    fn a_blank_line_between_two_parts_is_kept() {
        // A signature, a gap, then prose: closing the gap makes them one paragraph.
        let text = "fn main()\n\nRuns the program.";
        assert_eq!(tidy(text), "fn main()\n\nRuns the program.");
    }

    #[test]
    fn several_blank_lines_become_one() {
        assert_eq!(tidy("a\n\n\n\nb"), "a\n\nb");
    }

    #[test]
    fn the_ends_are_trimmed() {
        assert_eq!(tidy("\n\n  hello  \n\n"), "hello");
    }

    #[test]
    fn heading_marks_and_rules_come_out() {
        assert_eq!(tidy("# Title\n---\nbody"), "Title\nbody");
    }

    #[test]
    fn nothing_readable_is_nothing() {
        assert!(tidy("```\n```").is_empty());
        assert!(tidy("").is_empty());
        assert!(tidy("\n\n\n").is_empty());
    }
}
