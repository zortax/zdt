//! Putting the model onto the real keyboard.
//!
//! One effect, and the only thing in the application that gives a node focus. Every region says how
//! it takes the keyboard and none of them takes it for itself, so two regions cannot arm two timers
//! in one flush and leave the later one to win.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use zgui::prelude::Signal;
use zgui::reactive::RenderEffect;
use zgui::reactive::prelude::*;
use zgui::view::time::{TimeoutHandle, Timers};

use super::{Focus, Focusing, Sink, Spot};
use crate::workspace::Workspace;

/// The projector and the watcher, held for a window's life.
///
/// Dropping it stops both. A timer whose handle is dropped is cancelled, which is what stops a
/// claim landing on a window that has gone.
pub struct Projection {
    _projecting: RenderEffect<()>,
    _watching: RenderEffect<()>,
}

/// Starts putting the keyboard where the model says it is.
///
/// One per window over a session. `showing` says whether this session is the one on screen: a
/// window keeps several mounted and hidden, and a session nobody is looking at must neither take
/// the keyboard nor answer for it.
#[must_use]
pub fn project(
    focus: &Focusing,
    workspace: &Workspace,
    showing: Signal<bool, zgui::reactive::LocalStorage>,
) -> Projection {
    let timers = Timers::current();
    let claim: Rc<RefCell<Option<TimeoutHandle>>> = Rc::new(RefCell::new(None));

    let projecting = {
        let (focus, workspace, claim) = (focus.clone(), workspace.clone(), Rc::clone(&claim));
        RenderEffect::new(move |_| {
            // Read all three first. A region registering after it was focused is what has to wake
            // this, and it is the one thing that changes without the focus changing. Whether this
            // session is on screen is the other: switching to one changes no focus model at all,
            // and the keyboard has to be put back on whatever it says.
            let _ = focus.revision();
            let _ = workspace.mounted_revision();
            // Which buffer the focused window is showing. `]b` onto one already mounted moves no
            // focus and changes no revision, while it puts the keyboard on a different element
            // from the one holding it.
            if let Focus::Window(window) = focus.current() {
                let _ = workspace.window(window).and_then(|state| state.current);
            }
            let shown = showing.get();

            // A hidden session leaves the keyboard alone. Several are mounted at once, and one
            // nobody is looking at would take it from the one they are.
            if !shown {
                return;
            }

            let Some(sink) = sink_of(focus.current(), &focus, &workspace) else {
                // No sink is a real answer. A layer that draws no input takes the keys and leaves
                // the caret where it was, which is what documentation and suggestions want.
                return;
            };
            let Some(timers) = timers.as_ref() else {
                return;
            };
            // From a timer, because a node that is not mounted cannot take focus. One handle, so a
            // second decision in the same flush cancels the first rather than racing it.
            *claim.borrow_mut() = Some(timers.set_timeout(Duration::ZERO, move || sink.focus()));
        })
    };

    let watching = {
        let (focus, workspace) = (focus.clone(), workspace.clone());
        RenderEffect::new(move |_| {
            // The same rule: a hidden session has no business asking for the keyboard back.
            if showing.get() {
                watch(&focus, &workspace);
            }
        })
    };

    Projection {
        _projecting: projecting,
        _watching: watching,
    }
}

/// Says when the keyboard has fallen out of everything, and asks for it back.
///
/// Never writes the model from what it finds. Real focus belongs to a window and the model belongs
/// to a session, so two windows over one session have two answers and one model; a window nobody is
/// looking at would keep overwriting the one they are. What it does answer is focus leaving the
/// tree altogether, which a dragged divider and a library control that is a tab stop both do.
fn watch(focus: &Focusing, workspace: &Workspace) {
    if zgui::view::focused_node().get().is_some() {
        return;
    }
    // Nothing has it. Whatever took it has gone, so the model says where it belongs.
    if sink_of(focus.current_untracked(), focus, workspace).is_some() {
        focus.reproject();
    }
}

/// Which spot a focus names, and how the keyboard reaches it.
fn sink_of(wanted: Focus, focus: &Focusing, workspace: &Workspace) -> Option<Sink> {
    match wanted {
        Focus::Tree => focus.sink_for(Spot::Tree),
        Focus::Overlay(overlay) => focus.sink_for(Spot::Overlay(overlay)),
        Focus::Window(window) => {
            let buffer = workspace.buffer_in_untracked(window)?;
            // An editor is already filed with the workspace, which every action reads it from. A
            // terminal or a panel says how it takes the keyboard itself.
            workspace
                .handle_for(window, buffer)
                .map(Sink::Editor)
                .or_else(|| focus.sink_for(Spot::Buffer(window, buffer)))
        }
    }
}
