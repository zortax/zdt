//! The pickers, driven without a window.
//!
//! The ranking and the searching are asserted in `zdt-core`, against lists and directories. What is
//! asserted here is the layer above: that a source gathers what it says it does, that the caret
//! wraps, that a query narrows, and that choosing a row does the thing the row stands for.

use std::path::PathBuf;

use zdt::picker::{Picker, Source, Target};
use zdt::settings::Settings;
use zdt::workspace::Workspace;
use zdt_core::{Config, Project};
use zgui_testkit_view::Window;

/// A small project that removes itself.
struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "zdt-picker-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).expect("a directory");
        std::fs::write(root.join("src/main.rs"), "fn main() {\n    alpha();\n}\n").expect("a file");
        std::fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").expect("a file");
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"thing\"\n").expect("a file");
        Self(root)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A picker over `root`, with the workspace and settings it needs.
fn mount(window: &Window, root: &std::path::Path) -> Picker {
    window.scope.with(|| {
        let workspace = Workspace::new(Project::at(root));
        let settings = Settings::new(Config::default(), None);
        zdt::workspace::provide(workspace.clone());
        zdt::settings::provide(settings.clone());
        let picker = Picker::new(workspace, settings);
        zdt::picker::provide(picker.clone());
        picker
    })
}

/// Runs whatever the picker started, until it has finished.
///
/// Two clocks have to move: the harness's own, so that the debounce and the polling timers fire,
/// and the wall clock, so that the workers doing the walking and the searching get anywhere.
fn settle(window: &Window) {
    for _ in 0..200 {
        window.advance(std::time::Duration::from_millis(5));
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// What the picker is showing, as labels.
fn labels(picker: &Picker) -> Vec<String> {
    picker.rows().into_iter().map(|row| row.label).collect()
}

#[test]
fn the_file_picker_lists_the_project() {
    let temp = Temp::new("files");
    let window = Window::open();
    let picker = mount(&window, &temp.0);

    window.scope.with(|| {
        picker.open(Source::Files {
            reach: Default::default(),
        });
    });
    settle(&window);

    let mut found = labels(&picker);
    found.sort();
    assert_eq!(
        found,
        vec![
            "Cargo.toml".to_owned(),
            "src/lib.rs".to_owned(),
            "src/main.rs".to_owned()
        ]
    );
    assert!(picker.is_open());
}

#[test]
fn typing_narrows_the_file_list() {
    let temp = Temp::new("narrow");
    let window = Window::open();
    let picker = mount(&window, &temp.0);

    window.scope.with(|| {
        picker.open(Source::Files {
            reach: Default::default(),
        });
    });
    settle(&window);

    window.scope.with(|| picker.set_query("lib"));
    settle(&window);

    assert_eq!(labels(&picker), vec!["src/lib.rs".to_owned()]);
    assert_eq!(picker.counts().0, 1, "and says how many matched");
}

#[test]
fn a_row_carries_the_file_it_stands_for() {
    let temp = Temp::new("target");
    let window = Window::open();
    let picker = mount(&window, &temp.0);

    window.scope.with(|| {
        picker.open(Source::Files {
            reach: Default::default(),
        });
        picker.set_query("main");
    });
    settle(&window);

    let row = picker.selected().expect("a row");
    assert_eq!(
        row.target,
        Target::File {
            path: temp.0.join("src/main.rs"),
            line: None
        }
    );
}

#[test]
fn the_caret_wraps_rather_than_stopping() {
    let temp = Temp::new("wrap");
    let window = Window::open();
    let picker = mount(&window, &temp.0);

    window.scope.with(|| {
        picker.open(Source::Files {
            reach: Default::default(),
        });
    });
    settle(&window);

    let count = picker.len();
    assert_eq!(count, 3);

    assert_eq!(picker.at(), 0);
    picker.move_by(-1);
    assert_eq!(
        picker.at(),
        count - 1,
        "up from the first wraps to the last"
    );
    picker.move_by(1);
    assert_eq!(picker.at(), 0, "and back again");
}

#[test]
fn a_search_finds_lines_and_says_which() {
    let temp = Temp::new("grep");
    let window = Window::open();
    let picker = mount(&window, &temp.0);

    window.scope.with(|| {
        picker.open(Source::Grep {
            reach: Default::default(),
            start: String::new(),
        });
    });
    settle(&window);
    assert!(picker.is_empty(), "an empty query searches for nothing");

    window.scope.with(|| picker.set_query("alpha"));
    settle(&window);

    let rows = picker.rows();
    assert_eq!(rows.len(), 2, "one in each file");
    for row in &rows {
        let Target::File { line, .. } = &row.target else {
            panic!("a hit stands for a file at a line");
        };
        assert!(line.is_some());
        assert!(row.detail.contains("alpha"), "and shows the line");
    }
}

#[test]
fn a_new_query_replaces_the_last_search() {
    let temp = Temp::new("restart");
    let window = Window::open();
    let picker = mount(&window, &temp.0);

    window.scope.with(|| {
        picker.open(Source::Grep {
            reach: Default::default(),
            start: String::new(),
        });
        picker.set_query("alpha");
    });
    settle(&window);
    assert_eq!(picker.len(), 2);

    window.scope.with(|| picker.set_query("package"));
    settle(&window);

    let rows = picker.rows();
    assert_eq!(rows.len(), 1, "the earlier hits are gone, not added to");
    assert_eq!(rows[0].label, "Cargo.toml");
}

#[test]
fn the_buffer_picker_lists_what_is_open() {
    let temp = Temp::new("buffers");
    let window = Window::open();
    let picker = mount(&window, &temp.0);

    window.scope.with(|| {
        let workspace = zdt::workspace::use_workspace();
        workspace.open_document(
            Some(temp.0.join("src/main.rs")),
            zgui_editor::Document::new("fn main() {}"),
        );
        picker.open(Source::Buffers);
    });
    settle(&window);

    let found = labels(&picker);
    assert!(
        found.contains(&"src/main.rs".to_owned()),
        "the open file is there, by its path inside the project: {found:?}"
    );
}

#[test]
fn the_theme_picker_offers_the_built_in_ones() {
    let temp = Temp::new("themes");
    let window = Window::open();
    let picker = mount(&window, &temp.0);

    window.scope.with(|| picker.open(Source::Themes));
    settle(&window);

    assert!(labels(&picker).contains(&"oldworld".to_owned()));
    assert!(
        matches!(
            picker.selected().expect("a row").target,
            Target::Theme(ref name) if name == "oldworld"
        ),
        "and choosing one switches to it"
    );
}

#[test]
fn closing_it_stops_everything_it_started() {
    let temp = Temp::new("close");
    let window = Window::open();
    let picker = mount(&window, &temp.0);

    window.scope.with(|| {
        picker.open(Source::Grep {
            reach: Default::default(),
            start: String::new(),
        });
        picker.set_query("alpha");
        picker.close();
    });
    settle(&window);

    assert!(!picker.is_open());
    assert!(picker.is_empty(), "nothing arrives after it has closed");
    assert!(!picker.is_working());
}

#[test]
fn nothing_matched_is_an_empty_list_rather_than_the_whole_one() {
    let temp = Temp::new("nomatch");
    let window = Window::open();
    let picker = mount(&window, &temp.0);

    window.scope.with(|| {
        picker.open(Source::Files {
            reach: Default::default(),
        });
        picker.set_query("zzzzzzzz");
    });
    settle(&window);

    assert!(picker.is_empty());
    assert!(picker.selected().is_none());
}
