//! The drawing a `.excalidraw` buffer holds, and the editor over it.
//!
//! One document, two views. The buffer's text is the drawing; the editor works on a parse of it and
//! writes every change back as one replacement through the buffer's own history. So `u` in the
//! source view takes a drag back, the dirty mark follows, and `:w` writes what both views show.
//!
//! The replacement is trimmed to what actually differs, so moving one rectangle rewrites a few
//! lines rather than the whole file.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use rustc_hash::FxHashMap;
use zdt_excalidraw::{Board, EditorProps};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RenderEffect, RwSignal};
use zgui::view::time::Timers;
use zgui::{component, view};

use crate::workspace::{BufferId, WindowId, Workspace, use_workspace};

/// The keymap overlay a focused drawing answers keys in.
pub const REGION: &str = zdt_excalidraw::REGION;

/// How long typing settles before the drawing is read again.
const REPARSE_DEBOUNCE: Duration = Duration::from_millis(300);

/// One mounted drawing.
#[derive(Clone, Copy)]
pub struct Held {
    /// The editor.
    pub board: Board,
    /// The revision of the editor's own last write. The debounce skips it.
    pub expected: RwSignal<Option<u64>, LocalStorage>,
}

impl PartialEq for Held {
    fn eq(&self, other: &Self) -> bool {
        self.board == other.board && self.expected == other.expected
    }
}

/// Every mounted drawing, by the window and buffer it belongs to.
///
/// No signal, the same reasoning as the other rich registries: nothing on screen is decided by
/// which drawings exist, and a key that works one needs it right now.
#[derive(Clone)]
pub struct Drawings {
    inner: Rc<RefCell<FxHashMap<(WindowId, BufferId), Held>>>,
}

impl Drawings {
    /// Nothing mounted.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(FxHashMap::default())),
        }
    }

    fn register(&self, window: WindowId, buffer: BufferId, held: Held) {
        self.inner.borrow_mut().insert((window, buffer), held);
    }

    fn forget(&self, window: WindowId, buffer: BufferId, held: Held) {
        let mut kept = self.inner.borrow_mut();
        if kept.get(&(window, buffer)) == Some(&held) {
            kept.remove(&(window, buffer));
        }
    }

    /// The drawing the keyboard is in, when it is in one.
    fn current(&self, workspace: &Workspace) -> Option<(BufferId, Held)> {
        let window = workspace.focused_untracked();
        let buffer = workspace.buffer_in_untracked(window)?;
        if !workspace.is_rich_untracked(window, buffer) {
            return None;
        }
        let held = self.inner.borrow().get(&(window, buffer)).copied()?;
        Some((buffer, held))
    }
}

impl Default for Drawings {
    fn default() -> Self {
        Self::new()
    }
}

/// Puts the registry where every component can find it.
pub fn provide(drawings: Drawings) {
    zgui::reactive::provide_local_context(drawings);
}

fn use_drawings() -> Option<Drawings> {
    zgui::reactive::use_local_context::<Drawings>()
}

/// Working the drawing under the keyboard, from the keys of its region.
pub fn run(workspace: &Workspace, leaf: &str) {
    let Some(drawings) = use_drawings() else {
        return;
    };
    let Some((buffer, held)) = drawings.current(workspace) else {
        return;
    };
    if zdt_excalidraw::actions::run(&held.board, leaf) {
        write(workspace, buffer, held);
    }
}

/// Writes what the editor holds back into the buffer's text.
///
/// The replacement covers only the run of bytes that differ, so a change to one element leaves the
/// rest of the file's bytes exactly where they were.
fn write(workspace: &Workspace, buffer: BufferId, held: Held) {
    let Some(document) = workspace
        .buffer_untracked(buffer)
        .and_then(|entry| entry.document().cloned())
    else {
        return;
    };
    let Ok(next) = held.board.read_untracked().to_string() else {
        held.board
            .notice
            .set(Some("the drawing could not be written".to_owned()));
        return;
    };
    let current = document.rope().to_string();
    let Some(replacement) = difference(&current, &next) else {
        return;
    };
    if !document.apply(vec![replacement]) {
        return;
    }
    held.expected.set(Some(document.revision()));
}

/// The one replacement that turns `current` into `next`.
///
/// Answers nothing when they are the same. What the two share at either end is left alone, which is
/// what keeps a save a small diff.
fn difference(current: &str, next: &str) -> Option<(std::ops::Range<usize>, String)> {
    if current == next {
        return None;
    }
    let head = current
        .as_bytes()
        .iter()
        .zip(next.as_bytes())
        .take_while(|(a, b)| a == b)
        .count();
    // Kept on a character boundary, so the range is one a rope can be cut at.
    let head = floor_boundary(current, floor_boundary(next, head));

    let tail = current.as_bytes()[head..]
        .iter()
        .rev()
        .zip(next.as_bytes()[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let end = current.len() - tail;
    let end = ceil_boundary(current, end.max(head));
    let next_end = ceil_boundary(next, (next.len() - tail).max(head));

    Some((head..end, next[head..next_end].to_owned()))
}

/// `at`, moved back to the nearest character boundary.
fn floor_boundary(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// `at`, moved on to the nearest character boundary.
fn ceil_boundary(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while at < text.len() && !text.is_char_boundary(at) {
        at += 1;
    }
    at
}

/// Reads the buffer's text into the editor.
fn refresh(held: Held, document: &zgui_editor::Document, first: bool) {
    let text = document.rope().to_string();
    match excalidraw::file::parse(&text) {
        Ok(drawing) => {
            held.board.notice.set(None);
            let now = now_ms();
            let seed = fresh_seed();
            held.board.with_scene(|scene| {
                *scene = excalidraw::Scene::new(drawing, seed, now);
            });
            if first {
                zdt_excalidraw::actions::fit(&held.board);
            }
        }
        Err(error) => held.board.notice.set(Some(error.to_string())),
    }
}

/// The clock a change is stamped with.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|held| u64::try_from(held.as_millis()).unwrap_or(0))
        .unwrap_or(0)
}

/// A seed for the ids and wobbles a session makes.
fn fresh_seed() -> u32 {
    // The clock, which is different every time a drawing is opened and the same for every element
    // in one session.
    let held = now_ms();
    #[allow(clippy::cast_possible_truncation)]
    let held = (held as u32) & 0x7fff_ffff;
    held.max(1)
}

/// The drawing a `.excalidraw` buffer holds.
#[component]
pub fn ExcalidrawPreview(
    /// Which window it is in.
    window: WindowId,
    /// Which buffer it draws.
    buffer: BufferId,
) -> impl IntoView {
    use zdt_view::Erase;

    let workspace = use_workspace();
    let Some(entry) = workspace.buffer_untracked(buffer) else {
        // The buffer closed between the toggle and this mounting. Nothing to show.
        return view! { box() }.any();
    };
    let Some(document) = entry.document().cloned() else {
        return view! { box() }.any();
    };

    // Where the keyboard lands while the split is in rich form.
    let node = NodeRef::new();
    crate::focus::claim::sink(
        crate::focus::Spot::Buffer(window, buffer),
        crate::focus::Sink::Node(node),
    );

    let held = Held {
        board: Board::new(excalidraw::Scene::empty(fresh_seed(), now_ms())),
        expected: RwSignal::new_local(None),
    };
    refresh(held, &document, true);

    let drawings = use_drawings();
    if let Some(drawings) = &drawings {
        drawings.register(window, buffer, held);
    }
    on_cleanup_local({
        let drawings = drawings.clone();
        move || {
            if let Some(drawings) = &drawings {
                drawings.forget(window, buffer, held);
            }
        }
    });

    // Which surface it is drawn on. The setting decides, and when the setting leaves it to the
    // desktop the editor's own probe answers.
    {
        let settings = crate::settings::use_settings();
        let following = RenderEffect::new(move |_| {
            let scheme = settings.with(|config| config.ui.scheme);
            held.board.scheme.set(match scheme {
                zdt_core::config::Scheme::Light => zdt_excalidraw::state::Scheme::Light,
                zdt_core::config::Scheme::Dark => zdt_excalidraw::state::Scheme::Dark,
                zdt_core::config::Scheme::System => zdt_excalidraw::state::Scheme::System,
            });
        });
        on_cleanup_local(move || drop(following));
    }

    // Typing reaches the drawing after a pause; the editor's own writes are already in it.
    {
        let following = follow(&workspace, &document, held, window, buffer, entry.revision);
        on_cleanup_local(move || drop(following));
    }

    // Every change the editor makes goes back into the text.
    let sink = {
        let workspace = workspace.clone();
        zdt_excalidraw::Sink(Rc::new(move |_: &excalidraw::Scene| {
            write(&workspace, buffer, held);
        }))
    };

    // The keys. Everything the editor answers goes through the region's keymap, with the base map
    // layered underneath, so the pill's toggle and the window keys keep working.
    let vim = crate::vim::use_vim();
    let on_key = move |event: &mut EventCx<'_, events::KeyDown>| {
        // Words being typed take the keys first: while they are open the drawing has none of its
        // own, and the editor rather than a focused box is what the keyboard is pointed at.
        match zdt_excalidraw::text::key(&held.board, &event.key, event.modifiers.shift()) {
            zdt_excalidraw::text::Typed::Idle => {}
            zdt_excalidraw::text::Typed::Taken | zdt_excalidraw::text::Typed::Finished => {
                event.prevent_default();
                event.stop_propagation();
                return;
            }
        }
        if let Some(chord) = crate::keys::chord_of(event, event.modifiers)
            && vim.key_in_region(chord, REGION)
        {
            event.prevent_default();
        }
        event.stop_propagation();
    };

    view! {
        column(
            class = "exview",
            node_ref = node,
            tabindex = Focus::Programmatic,
            a11y:role = Role::Document,
            a11y:label = "Drawing",
            on:key_down = on_key,
            on:text = move |event: &mut EventCx<'_, events::Text>| {
                if zdt_excalidraw::text::insert(&held.board, &event.text) {
                    event.prevent_default();
                    event.stop_propagation();
                }
            }
        ) {
            Editor(board = held.board, sink = sink)
        }
    }
    .any()
}

/// Starts the debounced reread. The returned effect is dropped to stop it.
fn follow(
    workspace: &Workspace,
    document: &zgui_editor::Document,
    held: Held,
    window: WindowId,
    buffer: BufferId,
    revision: RwSignal<u64, LocalStorage>,
) -> RenderEffect<(u64, bool)> {
    let timers = Timers::current();
    let pending: Rc<RefCell<Option<zgui::view::time::TimeoutHandle>>> = Rc::new(RefCell::new(None));
    let stale = Rc::new(std::cell::Cell::new(false));
    let workspace = workspace.clone();
    let document = document.clone();

    RenderEffect::new(move |previous: Option<(u64, bool)>| {
        let at = revision.get();
        let showing = workspace.is_rich(window, buffer);
        let Some((was_at, _)) = previous else {
            // The mount read this text already.
            return (at, showing);
        };
        let moved = at != was_at;
        // The editor's own write put this text there.
        if moved && held.expected.get_untracked() == Some(at) {
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
                    refresh(held, &document, false);
                }));
            }
        }
        (at, showing)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_to_one_line_replaces_only_that_line() {
        let current = "{\n  \"a\": 1,\n  \"b\": 2\n}";
        let next = "{\n  \"a\": 9,\n  \"b\": 2\n}";
        let (range, text) = difference(current, next).expect("a change");
        assert!(range.len() <= 2, "the range is {range:?}");
        assert_eq!(text, "9");
        // And applying it gives the new text.
        let mut out = current.to_owned();
        out.replace_range(range, &text);
        assert_eq!(out, next);
    }

    #[test]
    fn the_same_text_is_no_change_at_all() {
        assert!(difference("hello", "hello").is_none());
    }

    #[test]
    fn text_added_at_the_end_is_one_insertion() {
        let (range, text) = difference("abc", "abcdef").expect("a change");
        assert_eq!(range, 3..3);
        assert_eq!(text, "def");
    }

    #[test]
    fn text_taken_from_the_middle_is_one_deletion() {
        let (range, text) = difference("abcdef", "abef").expect("a change");
        let mut out = "abcdef".to_owned();
        out.replace_range(range, &text);
        assert_eq!(out, "abef");
    }

    #[test]
    fn a_change_beside_a_letter_of_more_than_one_byte_stays_on_a_boundary() {
        let current = "aé b";
        let next = "aé c";
        let (range, text) = difference(current, next).expect("a change");
        assert!(current.is_char_boundary(range.start));
        assert!(current.is_char_boundary(range.end));
        let mut out = current.to_owned();
        out.replace_range(range, &text);
        assert_eq!(out, next);
    }

    /// The one thing that makes the drawing and its source one document: a change the editor makes
    /// is a step in the buffer's own history, so `u` in the source view takes it back.
    #[test]
    fn a_change_from_the_editor_is_one_step_of_the_buffers_history() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let before = r#"{
  "type": "excalidraw",
  "version": 2,
  "elements": [
    {
      "id": "a",
      "type": "rectangle",
      "x": 0,
      "y": 0
    }
  ]
}"#;
            let document = zgui_editor::Document::new(before);
            assert_eq!(document.with_history(|held| held.undo_depth()), 0);

            let next = before.replace("\"x\": 0", "\"x\": 40");
            let change = difference(before, &next).expect("a change");
            assert!(document.apply(vec![change]));

            assert_eq!(document.rope().to_string(), next);
            assert_eq!(
                document.with_history(|held| held.undo_depth()),
                1,
                "one change is one step"
            );
        });
    }

    #[test]
    fn a_whole_file_replaced_still_works() {
        let (range, text) = difference("abc", "xyz").expect("a change");
        assert_eq!(range, 0..3);
        assert_eq!(text, "xyz");
    }
}
