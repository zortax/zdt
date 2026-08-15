//! What is open, and what happens to it.
//!
//! The workspace holds signals, so it needs a reactive runtime to exist in — the testkit window
//! is the smallest one there is. No view is built: what is asserted is the state every part of
//! the interface reads, not how any of them draws it.

use zdt_core::Project;
use zgui_testkit_view::Window;

/// Runs `body` inside a reactive scope.
fn in_scope<R>(body: impl FnOnce() -> R) -> R {
    let window = Window::open();
    window.scope.with(body)
}

fn workspace() -> zdt::workspace::Workspace {
    zdt::workspace::Workspace::new(Project::at("/project"))
}

#[test]
fn a_new_workspace_has_somewhere_to_put_the_caret() {
    in_scope(|| {
        let space = workspace();
        assert_eq!(space.order().len(), 1, "one scratch buffer");
        assert!(space.current_buffer().is_some());
        assert!(space.layout().is_single());
    });
}

#[test]
fn opening_a_file_twice_shows_the_one_that_is_open() {
    // Otherwise every jump to an open file would throw away its undo history.
    in_scope(|| {
        let space = workspace();
        let one = space.open_document(
            Some("/project/a.rs".into()),
            zgui_editor::Document::new("first"),
        );
        let two = space.open_document(
            Some("/project/a.rs".into()),
            zgui_editor::Document::new("second"),
        );
        assert_eq!(one, two);
        assert_eq!(space.order().len(), 2, "the scratch buffer and the file");
        assert_eq!(
            space
                .buffer_untracked(one)
                .and_then(|buffer| buffer.document().map(zgui_editor::Document::text)),
            Some("first".to_owned()),
            "the text that was already open"
        );
    });
}

#[test]
fn the_buffer_line_walks_and_wraps() {
    in_scope(|| {
        let space = workspace();
        let a = space.open_document(Some("/project/a.rs".into()), zgui_editor::Document::new(""));
        let b = space.open_document(Some("/project/b.rs".into()), zgui_editor::Document::new(""));
        let order = space.order();
        assert_eq!(order.len(), 3);

        // Currently on b, the last of the three.
        space.cycle_buffer(1);
        assert_eq!(
            space.current_buffer().map(|buffer| buffer.id),
            Some(order[0])
        );
        space.cycle_buffer(-1);
        assert_eq!(space.current_buffer().map(|buffer| buffer.id), Some(b));
        space.cycle_buffer(-1);
        assert_eq!(space.current_buffer().map(|buffer| buffer.id), Some(a));
    });
}

#[test]
fn the_alternate_is_the_one_before_this_one() {
    in_scope(|| {
        let space = workspace();
        let a = space.open_document(Some("/project/a.rs".into()), zgui_editor::Document::new(""));
        let b = space.open_document(Some("/project/b.rs".into()), zgui_editor::Document::new(""));

        space.show_alternate();
        assert_eq!(space.current_buffer().map(|buffer| buffer.id), Some(a));
        space.show_alternate();
        assert_eq!(space.current_buffer().map(|buffer| buffer.id), Some(b));
    });
}

#[test]
fn closing_the_shown_buffer_shows_another() {
    in_scope(|| {
        let space = workspace();
        let a = space.open_document(Some("/project/a.rs".into()), zgui_editor::Document::new(""));
        assert!(space.close_buffer(a));
        assert!(
            space
                .find_path(std::path::Path::new("/project/a.rs"))
                .is_none()
        );
        assert!(
            space.current_buffer().is_some(),
            "the window falls back to the buffer beside the one that went"
        );
    });
}

#[test]
fn closing_the_last_buffer_leaves_the_window_empty() {
    // A window with nothing in it is a real state. Conjuring a scratch buffer instead would put a
    // file nobody asked for on the buffer line every time the last one was closed.
    in_scope(|| {
        let space = workspace();
        let only = space.order()[0];
        assert!(space.close_buffer(only));

        assert!(space.order().is_empty());
        assert!(space.current_buffer().is_none());
        assert!(
            space.window(space.focused_untracked()).is_some(),
            "the window is still there — it is showing nothing, not gone"
        );
    });
}

#[test]
fn opening_something_fills_an_empty_window_again() {
    in_scope(|| {
        let space = workspace();
        let only = space.order()[0];
        space.close_buffer(only);

        let id = space.open_document(Some("/project/a.rs".into()), zgui_editor::Document::new(""));
        assert_eq!(space.current_buffer().map(|buffer| buffer.id), Some(id));
    });
}

#[test]
fn a_buffer_opens_holding_what_is_on_disk() {
    in_scope(|| {
        let space = workspace();
        let id = space.open_document(
            Some("/project/a.rs".into()),
            zgui_editor::Document::new("one\n"),
        );
        let buffer = space.buffer_untracked(id).expect("it is open");
        assert!(
            !buffer.is_dirty(),
            "what it opens with is what was read, so nothing differs yet"
        );

        buffer.mark_saved();
        assert!(!buffer.is_dirty());
    });
}

#[test]
fn splitting_and_closing_a_window_keeps_one_on_screen() {
    in_scope(|| {
        let space = workspace();
        let shown = space.current_buffer().expect("something is shown").id;

        let new = space
            .split(zdt::workspace::Axis::Horizontal)
            .expect("it split");
        assert_eq!(space.layout().windows().len(), 2);
        assert_eq!(space.focused(), new, "the new window takes the keyboard");
        assert_eq!(
            space.buffer_in_untracked(new),
            Some(shown),
            "a split shows the same buffer in both"
        );

        assert!(space.close_window());
        assert!(space.layout().is_single());
        assert!(!space.close_window(), "the last window does not close");
    });
}

#[test]
fn moving_a_buffer_reorders_the_line_and_stops_at_the_ends() {
    in_scope(|| {
        let space = workspace();
        space.open_document(Some("/project/a.rs".into()), zgui_editor::Document::new(""));
        let b = space.open_document(Some("/project/b.rs".into()), zgui_editor::Document::new(""));

        space.move_buffer(-1);
        assert_eq!(space.order()[1], b);
        space.move_buffer(-1);
        assert_eq!(space.order()[0], b);
        space.move_buffer(-1);
        assert_eq!(space.order()[0], b, "it does not fall off the front");
    });
}
