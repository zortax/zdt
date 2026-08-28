//! Keeping the drawing in step with the text.
//!
//! The vector element draws whatever string the render signal holds. Typing refreshes it after a
//! pause, the markdown preview's shape. A commit from the editor refreshes it at once and leaves
//! its mark in `expected`, so the debounce does not parse the editor's own write a second time.

use std::rc::Rc;
use std::time::Duration;

use zgui::reactive::RenderEffect;
use zgui::reactive::prelude::*;
use zgui::view::time::Timers;

use super::SvgState;
use super::model::SvgModel;
use crate::workspace::{BufferId, WindowId, Workspace};

/// How long typing settles before the drawing is parsed again.
const REPARSE_DEBOUNCE: Duration = Duration::from_millis(300);

/// Reads the text and writes every derived signal on `state`.
pub fn refresh(state: &SvgState, document: &zgui_editor::Document) {
    let text = document.rope().to_string();
    let revision = document.revision();
    state
        .model
        .set(SvgModel::parse(&text, revision).map(Rc::new));
    state.notice.set(notice_of(&text));
    state.render.set(text);
}

/// What the corner has to say about the drawing, when anything.
fn notice_of(text: &str) -> Option<String> {
    match zgui_svg::parse(text) {
        Err(error) => Some(error.to_string()),
        Ok(document) => {
            let unsupported = document.unsupported();
            if unsupported.is_empty() {
                None
            } else {
                let mut parts = Vec::new();
                let mut part = |count: u32, what: &str| {
                    if count > 0 {
                        parts.push(format!("{count} {what}"));
                    }
                };
                part(unsupported.text, "text");
                part(unsupported.images, "image");
                part(unsupported.masks, "mask");
                part(unsupported.filters, "filter");
                part(unsupported.patterns, "pattern");
                part(unsupported.blend_modes, "blend");
                Some(format!("not drawn: {}", parts.join(", ")))
            }
        }
    }
}

/// Starts the debounced refresh. The returned effect is dropped to stop it.
pub fn follow(
    workspace: &Workspace,
    document: &zgui_editor::Document,
    state: SvgState,
    window: WindowId,
    buffer: BufferId,
    revision: zgui::reactive::RwSignal<u64, zgui::reactive::LocalStorage>,
) -> RenderEffect<(u64, bool)> {
    let timers = Timers::current();
    let pending: Rc<std::cell::RefCell<Option<zgui::view::time::TimeoutHandle>>> =
        Rc::new(std::cell::RefCell::new(None));
    let stale = Rc::new(std::cell::Cell::new(false));
    let workspace = workspace.clone();
    let document = document.clone();

    RenderEffect::new(move |previous: Option<(u64, bool)>| {
        let at = revision.get();
        let showing = workspace.is_rich(window, buffer);
        let Some((was_at, _)) = previous else {
            // The mount refresh is this text already.
            return (at, showing);
        };
        let moved = at != was_at;
        // The editor's own write refreshed everything as it landed.
        if moved && state.expected.get_untracked() == Some(at) {
            return (at, showing);
        }
        if moved && !showing {
            stale.set(true);
        }
        if showing && (moved || stale.get()) {
            stale.set(false);
            if let Some(timers) = timers.as_ref() {
                let document = document.clone();
                *pending.borrow_mut() = Some(timers.set_timeout(REPARSE_DEBOUNCE, move || {
                    refresh(&state, &document);
                }));
            }
        }
        (at, showing)
    })
}
