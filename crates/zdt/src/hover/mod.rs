//! What the server says about the thing under the caret.
//!
//! A panel anchored to the caret, which is what `point_for_byte` was added for.
//!
//! # Two states, and why
//!
//! `K` opens the panel. The panel takes no keys. The next thing typed goes where it would have
//! gone anyway and the panel closes. A documentation panel that swallows keys has to be dismissed
//! before work can continue.
//!
//! `K` again *focuses* it, and now it does take keys: `j`, `<C-d>`, `G` and `q`. The second press
//! means "I want to read this", and somebody who wants to read a panel of documentation needs to
//! scroll it. The file tree works the same way, by the same mechanism: a named keymap overlay in
//! front of the base map, only while the region has the keyboard.
//!
//! # The text
//!
//! Markdown, parsed and drawn. See [`crate::markdown`]. It used to be stripped to plain text,
//! which cost the one thing a hover is worth. The signature is in a fenced block, and a fenced
//! block shown as prose is a signature in one colour.

mod panel;

pub use crate::hover::panel::{HoverPanel, HoverPanelProps};

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

use crate::markdown::{Block, parse};

/// A density made safe to divide by: one of nothing would turn a length into infinity.
fn density(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0.01 {
        scale
    } else {
        1.0
    }
}

/// `device` device pixels as CSS pixels, which is what the keys count in.
fn css(scale: f32, device: f32) -> f32 {
    device / density(scale)
}

/// `css` CSS pixels as device pixels, which is what a scroll container is moved in.
fn device(scale: f32, css: f32) -> f32 {
    css * density(scale)
}

/// How far one `j` moves the panel, in pixels.
///
/// One line, so that walking down a hover with `j` moves it the same distance as walking down the
/// file with `j` does.
const LINE: f32 = 16.0;

/// What is being shown, and where.
#[derive(Clone, PartialEq, Debug)]
pub struct Showing {
    /// The documentation, parsed.
    pub blocks: Vec<Block>,
    /// Where the caret was.
    pub caret: zgui_editor::CaretRect,
}

/// The one hover panel.
#[derive(Clone, Copy)]
pub struct Hover {
    showing: RwSignal<Option<Showing>, LocalStorage>,
    /// Whether the panel has the keyboard.
    ///
    /// Separate from whether it is showing, because those are two different states with two
    /// different key behaviours and an editor that conflated them would be one where `K` made `j`
    /// stop moving the caret.
    focused: RwSignal<bool, LocalStorage>,
    /// How far down it has been scrolled, in pixels.
    offset: RwSignal<f32, LocalStorage>,
    /// How far it *can* be scrolled, which only a measured panel knows.
    limit: RwSignal<f32, LocalStorage>,
}

impl Hover {
    /// Nothing showing.
    #[must_use]
    pub fn new() -> Self {
        Self {
            showing: RwSignal::new_local(None),
            focused: RwSignal::new_local(false),
            offset: RwSignal::new_local(0.0),
            limit: RwSignal::new_local(0.0),
        }
    }

    /// Shows what a server answered, at the caret.
    pub fn show(&self, contents: &lsp_types::HoverContents, at: zgui_editor::CaretRect) {
        self.show_markdown(&markdown_of(contents), at);
    }

    /// The same, from markdown that came from somewhere else.
    pub fn show_markdown(&self, markdown: &str, at: zgui_editor::CaretRect) {
        let blocks = parse(markdown);
        if blocks.is_empty() {
            return;
        }
        self.offset.set(0.0);
        self.focused.set(false);
        self.showing.set(Some(Showing { blocks, caret: at }));
    }

    /// Puts it away.
    pub fn hide(&self) {
        if self.showing.with_untracked(Option::is_some) {
            self.showing.set(None);
        }
        if self.focused.get_untracked() {
            self.focused.set(false);
        }
    }

    /// Whether anything is showing, without subscribing.
    #[must_use]
    pub fn is_showing(&self) -> bool {
        self.showing.with_untracked(Option::is_some)
    }

    /// Whether it has the keyboard, without subscribing.
    #[must_use]
    pub fn is_focused(&self) -> bool {
        self.focused.get_untracked()
    }

    /// Gives it the keyboard, which the second `K` does.
    ///
    /// Answers whether it took it. False when there is nothing showing to read.
    pub fn focus(&self) -> bool {
        if !self.is_showing() {
            return false;
        }
        self.focused.set(true);
        true
    }

    /// What is showing. Tracked.
    #[must_use]
    pub fn showing(&self) -> Option<Showing> {
        self.showing.get()
    }

    /// How far down it is. Tracked.
    #[must_use]
    pub fn offset(&self) -> f32 {
        self.offset.get()
    }

    /// Says how far it can be scrolled, which the panel works out once it has been laid out.
    pub fn set_limit(&self, limit: f32) {
        let limit = limit.max(0.0);
        if (self.limit.get_untracked() - limit).abs() > 0.5 {
            self.limit.set(limit);
            // A panel that shrank under a scrolled view would otherwise be scrolled past its end.
            let offset = self.offset.get_untracked();
            if offset > limit {
                self.offset.set(limit);
            }
        }
    }

    /// Moves the view by `lines`, clamped to what there is.
    pub fn scroll_lines(&self, lines: f32) {
        self.scroll_by(lines * LINE);
    }

    /// Moves it by `pixels`, clamped to what there is.
    pub fn scroll_by(&self, pixels: f32) {
        let limit = self.limit.get_untracked();
        let next = (self.offset.get_untracked() + pixels).clamp(0.0, limit);
        if (next - self.offset.get_untracked()).abs() > f32::EPSILON {
            self.offset.set(next);
        }
    }

    /// To the top, which `gg` does.
    pub fn to_top(&self) {
        self.offset.set(0.0);
    }

    /// To the bottom, which `G` does.
    pub fn to_bottom(&self) {
        self.offset.set(self.limit.get_untracked());
    }

    /// How tall the panel is, for the keys that move by screenfuls.
    ///
    /// Never measured. The panel's own maximum height is a style-sheet number, and the keys that
    /// use it want "about a screen".
    #[must_use]
    pub const fn page(&self) -> f32 {
        240.0
    }
}

impl Default for Hover {
    fn default() -> Self {
        Self::new()
    }
}

/// One hover's contents, as markdown.
///
/// The four shapes the protocol allows, flattened into the one this editor draws. A
/// `LanguageString` is a fenced block that lost its fence on the way through the protocol, so it
/// gets the fence back. A Go hover uses that shape, and would otherwise show its signature as
/// prose.
#[must_use]
pub fn markdown_of(contents: &lsp_types::HoverContents) -> String {
    use lsp_types::{HoverContents, MarkedString};

    fn one(part: &MarkedString) -> String {
        match part {
            MarkedString::String(text) => text.clone(),
            MarkedString::LanguageString(block) => {
                format!("```{}\n{}\n```", block.language, block.value)
            }
        }
    }

    match contents {
        HoverContents::Scalar(part) => one(part),
        HoverContents::Markup(markup) => markup.value.clone(),
        HoverContents::Array(parts) => parts.iter().map(one).collect::<Vec<_>>().join("\n\n"),
    }
}

/// The first line worth reading out of a block of markdown.
///
/// For the status line, which has one line and no markup. Fences and blank lines are skipped: a
/// status line that says "```" is a status line that has said nothing.
#[must_use]
pub fn one_line(markdown: &str) -> String {
    markdown
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("```") && !line.starts_with("---"))
        .unwrap_or("")
        .to_owned()
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
