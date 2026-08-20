//! The file tree, driven the way a person drives it.
//!
//! The tree model itself is asserted in `zdt-core`, against a directory and nothing else. What is
//! asserted here is the layer above it: a key resolves against the tree's overlay and not the
//! editor's map, walking and opening move the caret where they should, and the filesystem
//! operations reach the disk.
//!
//! Every one of these is synchronous where it can be, and pumps the window's tasks where it
//! cannot. Reading a directory happens on a worker, so the assertion has to wait for it.

use std::path::{Path, PathBuf};

use zdt::explorer::Explorer;
use zdt_core::tree::Filter;
use zgui_testkit_view::Window;

/// A directory tree that removes itself.
struct Temp(PathBuf);

impl Temp {
    /// A few files and directories, enough to walk.
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "zdt-tree-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).expect("a directory");
        std::fs::create_dir_all(root.join("docs")).expect("a directory");
        std::fs::write(root.join("Cargo.toml"), "[package]\n").expect("a file");
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("a file");
        std::fs::write(root.join("src/lib.rs"), "").expect("a file");
        std::fs::write(root.join(".hidden"), "").expect("a file");
        Self(root)
    }

    fn path(&self, rest: &str) -> PathBuf {
        self.0.join(rest)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// An explorer over `root`, open, with its rows read.
///
/// A workspace comes with it, because where the keyboard is belongs to the session and the tree
/// reads its answer from there.
fn open(window: &Window, root: &Path) -> Explorer {
    let explorer = window.scope.with(|| {
        let workspace = zdt::workspace::Workspace::new(zdt_core::Project::at(root));
        let explorer = Explorer::new(root, Filter::default(), workspace.focus().clone());
        explorer.toggle();
        explorer
    });
    settle(window);
    explorer
}

/// Runs whatever the tree started on a worker, until it has finished.
///
/// A directory read is a task and a blocking call; both have to come back before the rows the
/// assertion is about exist.
fn settle(window: &Window) {
    for _ in 0..400 {
        window.frame();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// The names in the tree, in order, with their depth.
fn shape(explorer: &Explorer) -> Vec<(usize, String)> {
    explorer
        .rows()
        .into_iter()
        .map(|row| (row.depth, row.entry.name))
        .collect()
}

#[test]
fn opening_reads_the_root_with_directories_first() {
    let temp = Temp::new("root");
    let window = Window::open();
    let explorer = open(&window, &temp.0);

    assert_eq!(
        shape(&explorer),
        vec![
            (0, "docs".to_owned()),
            (0, "src".to_owned()),
            (0, "Cargo.toml".to_owned()),
        ],
        "directories come first, and the dotfile is not shown"
    );
}

#[test]
fn hidden_files_appear_when_the_filter_says_so() {
    let temp = Temp::new("hidden");
    let window = Window::open();
    let explorer = open(&window, &temp.0);

    explorer.set_filter(Filter {
        hidden: true,
        ignored: false,
    });
    settle(&window);

    assert!(
        shape(&explorer).iter().any(|(_, name)| name == ".hidden"),
        "the dotfile is shown once the filter allows it"
    );
}

#[test]
fn walking_stops_at_the_ends() {
    let temp = Temp::new("walk");
    let window = Window::open();
    let explorer = open(&window, &temp.0);

    assert_eq!(explorer.at(), 0);
    explorer.move_by(-1);
    assert_eq!(explorer.at(), 0, "up from the first row stays");

    explorer.move_by(100);
    assert_eq!(explorer.at(), explorer.len() - 1, "down stops at the last");
}

#[test]
fn opening_a_directory_shows_what_is_in_it() {
    let temp = Temp::new("expand");
    let window = Window::open();
    let explorer = open(&window, &temp.0);

    // `src` is the second row: directories first, alphabetically.
    explorer.go_to(1);
    assert_eq!(explorer.selected().expect("a row").entry.name, "src");

    assert!(
        explorer.open_selected().is_none(),
        "a directory opens, and answers no file to open"
    );
    settle(&window);

    assert_eq!(
        shape(&explorer),
        vec![
            (0, "docs".to_owned()),
            (0, "src".to_owned()),
            (1, "lib.rs".to_owned()),
            (1, "main.rs".to_owned()),
            (0, "Cargo.toml".to_owned()),
        ]
    );
}

#[test]
fn a_file_is_answered_rather_than_opened() {
    let temp = Temp::new("file");
    let window = Window::open();
    let explorer = open(&window, &temp.0);

    explorer.go_to(2);
    assert_eq!(
        explorer.open_selected(),
        Some(temp.path("Cargo.toml")),
        "the tree hands the file back; opening a buffer is not its business"
    );
}

#[test]
fn closing_a_directory_and_going_up() {
    let temp = Temp::new("collapse");
    let window = Window::open();
    let explorer = open(&window, &temp.0);

    explorer.go_to(1);
    explorer.open_selected();
    settle(&window);

    // On a child, `h` goes to the directory holding it.
    explorer.go_to(2);
    assert_eq!(explorer.selected().expect("a row").entry.name, "lib.rs");
    explorer.parent_or_close();
    assert_eq!(explorer.selected().expect("a row").entry.name, "src");

    // On an open directory, `h` closes it.
    explorer.parent_or_close();
    assert_eq!(
        shape(&explorer),
        vec![
            (0, "docs".to_owned()),
            (0, "src".to_owned()),
            (0, "Cargo.toml".to_owned()),
        ]
    );
}

#[test]
fn revealing_opens_the_way_to_a_file() {
    let temp = Temp::new("reveal");
    let window = Window::open();
    let explorer = open(&window, &temp.0);

    explorer.reveal(&temp.path("src/main.rs"));
    settle(&window);

    assert_eq!(
        explorer.selected().expect("a row").entry.path,
        temp.path("src/main.rs"),
        "the caret lands on the file, with its directory open"
    );
}

#[test]
fn the_target_directory_is_where_a_new_file_would_go() {
    let temp = Temp::new("target");
    let window = Window::open();
    let explorer = open(&window, &temp.0);

    // On a directory, into it.
    explorer.go_to(1);
    assert_eq!(explorer.target_directory(), temp.path("src"));

    // On a file, beside it.
    explorer.go_to(2);
    assert_eq!(explorer.target_directory(), temp.0);
}

#[test]
fn cutting_holds_a_path_and_pasting_forgets_it() {
    let temp = Temp::new("clipboard");
    let window = Window::open();
    let explorer = open(&window, &temp.0);

    explorer.go_to(2);
    assert_eq!(explorer.hold(true), Some(temp.path("Cargo.toml")));

    let held = explorer.clipboard().expect("something is held");
    assert!(held.cut);
    assert_eq!(held.path, temp.path("Cargo.toml"));

    explorer.release();
    assert!(explorer.clipboard().is_none());
}

#[test]
fn the_keyboard_follows_the_panel() {
    let temp = Temp::new("focus");
    let window = Window::open();
    let explorer = open(&window, &temp.0);

    assert!(explorer.is_open());
    assert!(explorer.is_focused(), "opening it puts the keyboard in it");

    explorer.unfocus();
    assert!(explorer.is_open(), "the panel stays");
    assert!(!explorer.is_focused());

    explorer.close();
    assert!(!explorer.is_open());
}
