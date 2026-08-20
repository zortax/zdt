//! Writing a session down and putting it back.
//!
//! The promise is that reopening a session leaves the editor as it was, so what is asserted here
//! is the *whole* value: the splits and their shares, the buffer line's order, the carets, the
//! scroll, the tree, and the undo history. Anything that only looks right is not enough.

use std::path::{Path, PathBuf};

use zdt::session::SessionKey;
use zdt::session::host::SessionHost;
use zdt::session::{capture, restore, schema};
use zgui_testkit_view::Window;

/// A project directory and a state directory, both of which remove themselves.
struct Place {
    project: PathBuf,
    state: zdt_core::state::State,
}

impl Place {
    fn new(name: &str) -> Self {
        let base = std::env::temp_dir().join(format!(
            "zdt-persist-{}-{name}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let project = base.join("project");
        std::fs::create_dir_all(&project).expect("the directory is made");
        Self {
            project: std::fs::canonicalize(&project).expect("it canonicalises"),
            state: zdt_core::state::State::at(base.join("state")),
        }
    }

    fn key(&self) -> SessionKey {
        SessionKey::of(&self.project).expect("it is a directory")
    }

    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.project.join(name);
        std::fs::write(&path, text).expect("it writes");
        path
    }
}

impl Drop for Place {
    fn drop(&mut self) {
        if let Some(base) = self.project.parent() {
            let _ = std::fs::remove_dir_all(base);
        }
    }
}

fn host(window: &Window) -> SessionHost {
    let root = zdt::session::host::detached_root();
    window.scope.with(|| {
        let global = zdt::app::global::install();
        let host = SessionHost::new(global, root);
        zdt::session::host::provide(host.clone());
        host
    })
}

/// Takes a snapshot of `session` the way the writer does.
fn snap(session: &zdt::session::Session) -> schema::Snapshot {
    let views = capture::Views::default();
    capture::capture(session, &views, 1)
}

#[test]
fn the_splits_and_their_shares_come_back() {
    let window = Window::open();
    let place = Place::new("splits");
    let host = host(&window);
    let session = host.session(host.open(place.key())).expect("it was made");

    let taken = window.scope.with(|| {
        let workspace = session.workspace();
        workspace.split(zdt::workspace::Axis::Vertical);
        workspace.split(zdt::workspace::Axis::Horizontal);
        let first = workspace.windows()[0];
        workspace.resize(first, &[70.0, 30.0]);
        snap(&session)
    });

    assert_eq!(taken.windows.len(), 3, "three splits were written down");
    // The tree is a division of divisions, and its shares are what a drag set.
    assert!(!taken.layout.children.is_empty(), "a division, not a leaf");

    // Put it back into a session that knows nothing.
    let other = Place::new("splits-back");
    let restored = host.session(host.open(other.key())).expect("it was made");
    let mut views = capture::Views::default();
    window
        .scope
        .with(|| restore::apply(&restored, &taken, &mut views));

    assert_eq!(restored.workspace().windows().len(), 3);
    let back = window.scope.with(|| snap(&restored));
    assert_eq!(back.windows.len(), taken.windows.len());
    assert_eq!(shares(&back.layout), shares(&taken.layout));
}

/// Every share in a layout, in the order they are written down.
fn shares(node: &schema::LayoutNode) -> Vec<i64> {
    node.children
        .iter()
        .flat_map(|child| {
            let mut found = vec![(child.share * 100.0).round() as i64];
            found.extend(shares(&child.node));
            found
        })
        .collect()
}

#[test]
fn the_buffer_line_comes_back_in_order() {
    let window = Window::open();
    let place = Place::new("order");
    let host = host(&window);
    let session = host.session(host.open(place.key())).expect("it was made");

    let (taken, paths) = window.scope.with(|| {
        let workspace = session.workspace();
        let mut paths = Vec::new();
        for name in ["one.txt", "two.txt", "three.txt"] {
            let path = place.file(name, "x");
            workspace.open_document(Some(path.clone()), zgui_editor::Document::new("x"));
            paths.push(path);
        }
        (snap(&session), paths)
    });

    let written: Vec<&Path> = taken
        .order
        .iter()
        .filter_map(|at| taken.buffers.get(*at as usize)?.path.as_deref())
        .collect();
    for path in &paths {
        assert!(
            written.contains(&path.as_path()),
            "{path:?} was written down"
        );
    }
    // And in the order they were opened in, which is the order the buffer line shows.
    let positions: Vec<usize> = paths
        .iter()
        .map(|path| {
            written
                .iter()
                .position(|held| *held == path.as_path())
                .expect("it is there")
        })
        .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn a_session_survives_a_trip_through_the_disk() {
    // The whole point: what is written is what comes back, byte for byte.
    let window = Window::open();
    let place = Place::new("disk");
    let host = host(&window);
    let session = host.session(host.open(place.key())).expect("it was made");

    let taken = window.scope.with(|| {
        session.workspace().open_document(
            Some(place.file("a.txt", "hello")),
            zgui_editor::Document::new("hello"),
        );
        session.workspace().split(zdt::workspace::Axis::Vertical);
        snap(&session)
    });

    let directory = zdt::session::store::directory_for(&place.state, &place.project);
    zdt::session::store::write_manifest(&directory, &taken).expect("it writes");

    let back = zdt::session::store::read(&place.state, &place.project).expect("it reads");
    assert_eq!(back.root, taken.root);
    assert_eq!(back.order, taken.order);
    assert_eq!(back.windows.len(), taken.windows.len());
    assert_eq!(back.buffers.len(), taken.buffers.len());
    assert_eq!(back.focused, taken.focused);
}

#[test]
fn a_caret_and_a_scroll_position_are_written_down() {
    let window = Window::open();
    let place = Place::new("carets");
    let host = host(&window);
    let session = host.session(host.open(place.key())).expect("it was made");

    let mut views = capture::Views::default();
    let taken = window.scope.with(|| {
        let workspace = session.workspace();
        let id = workspace.open_document(
            Some(place.file("a.txt", "one\ntwo\nthree\n")),
            zgui_editor::Document::new("one\ntwo\nthree\n"),
        );
        // No editor is mounted in a testkit window, so the view is recorded the way an unmounting
        // one records it: straight into the cache.
        views.insert(
            (workspace.focused_untracked(), id),
            schema::ViewSnapshot {
                window: 0,
                buffer: 0,
                selections: vec![schema::SelectionSnapshot { anchor: 8, head: 8 }],
                primary: 0,
                top_line: 1.5,
                x_px: 40.0,
            },
        );
        capture::capture(&session, &views, 1)
    });

    let view = taken.views.first().expect("one view was written down");
    assert_eq!(view.selections[0].head, 8);
    assert!((view.top_line - 1.5).abs() < f64::EPSILON);
    assert!((view.x_px - 40.0).abs() < f64::EPSILON);
}

#[test]
fn a_dirty_buffer_keeps_its_text_and_its_history() {
    // The two halves of "no difference noticeable": the unsaved text, and being able to undo it.
    let place = Place::new("dirty");
    let path = place.file("a.txt", "on disk\n");

    let content = schema::BufferContent {
        format: schema::FORMAT,
        text: Some("typed but not saved\n".to_owned()),
        dirty: true,
        history: schema::HistorySnapshot::default(),
        trimmed: false,
    };
    let directory = zdt::session::store::directory_for(&place.state, &place.project);
    let reference = zdt::session::store::write_blob(&directory, 0, &content).expect("it writes");

    let back = zdt::session::store::read_blob(&directory, &reference).expect("it reads");
    assert_eq!(back.text.as_deref(), Some("typed but not saved\n"));
    assert!(back.dirty);
    assert_eq!(
        std::fs::read_to_string(&path).expect("it reads"),
        "on disk\n"
    );
}

#[test]
fn a_file_that_changed_while_the_editor_was_closed_is_not_clobbered() {
    // Never clobber and never resurrect. What is on disk wins, and the unsaved text is kept back
    // rather than written over it.
    let place = Place::new("conflict");
    let path = place.file("a.txt", "before\n");
    let stored = capture::stamp(&path);

    std::fs::write(&path, "somebody else changed it\n").expect("it writes");
    let now = capture::stamp(&path);

    assert!(!stored.matches(&now), "the change is noticed");
    assert!(stored.matches(&stored), "an unchanged file is unchanged");
}

#[test]
fn a_file_that_was_only_touched_is_still_the_same_file() {
    // The time is advisory: a network share can move it backwards and a checkout can leave it.
    let place = Place::new("touched");
    let path = place.file("a.txt", "same\n");
    let one = capture::stamp(&path);
    let two = schema::DiskStamp {
        mtime_ms: one.mtime_ms + 10_000,
        ..one
    };
    assert!(one.matches(&two));
}

#[test]
fn the_tree_and_the_command_line_come_back() {
    let window = Window::open();
    let place = Place::new("tree");
    std::fs::create_dir_all(place.project.join("src")).expect("the directory is made");
    let host = host(&window);
    let session = host.session(host.open(place.key())).expect("it was made");

    let taken = window.scope.with(|| {
        session
            .cmdline()
            .restore_history(vec!["w".to_owned(), "q".to_owned()]);
        snap(&session)
    });

    assert_eq!(taken.cmdline.history, vec!["w".to_owned(), "q".to_owned()]);
    assert!(
        taken.tree.open.is_some(),
        "whether the panel was open is recorded"
    );
}

#[test]
fn vim_registers_survive_the_round_trip() {
    let window = Window::open();
    let place = Place::new("registers");
    let host = host(&window);
    let session = host.session(host.open(place.key())).expect("it was made");

    let taken = window.scope.with(|| {
        session.vim().restore_memory(
            vec![
                ("a".to_owned(), "yanked".to_owned(), false),
                (String::new(), "unnamed".to_owned(), true),
            ],
            Vec::new(),
            Vec::new(),
            0,
        );
        snap(&session)
    });

    let named: Vec<&str> = taken
        .vim
        .registers
        .iter()
        .map(|register| register.text.as_str())
        .collect();
    assert!(named.contains(&"yanked"));
    assert!(named.contains(&"unnamed"));
}
