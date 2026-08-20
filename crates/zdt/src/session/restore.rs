//! Putting a session back, in stages.
//!
//! Four constraints decide the order, and none of them can be worked around:
//!
//!   1. Buffers must exist before a split can name one.
//!   2. An [`EditorHandle`](zgui_editor::EditorHandle) only exists once a view has mounted, which
//!      is at least a frame after the buffer does.
//!   3. Reading a file blocks, so it happens on a worker.
//!   4. Expanding the tree reads directories, so that does too.
//!
//! So: the manifest and the splits come back synchronously, because the *first frame* has to be
//! the right one; the buffers that are actually on screen have their text read in the same pass,
//! for the same reason; everything else arrives behind, and the carets land as each editor
//! mounts.

use std::path::PathBuf;

use slotmap::Key;
use zgui::prelude::*;

use crate::session::Session;
use crate::session::capture::Views;
use crate::session::schema::{
    BufferSort, DiskStamp, LayoutChild, LayoutNode, PlaceSnapshot, SelectionSnapshot, Snapshot,
    SplitAxis, ViewSnapshot,
};
use crate::session::store;
use crate::workspace::{Axis, BufferId, BufferKind, Layout, Restored, WindowId};

/// What a restore could not do, for one line in the status line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// How many buffers came back.
    pub files: usize,
    /// Files that changed on disk while the editor was closed, whose unsaved text was kept back.
    pub conflicted: Vec<PathBuf>,
    /// Files that are no longer there.
    pub missing: Vec<PathBuf>,
    /// How many buffers had their undo history shortened to fit.
    pub trimmed: usize,
}

impl Report {
    /// One line saying what happened, or nothing when everything came back.
    #[must_use]
    pub fn say(&self) -> Option<String> {
        let mut parts = Vec::new();
        if !self.conflicted.is_empty() {
            parts.push(format!(
                "{} changed on disk; the unsaved text was kept",
                self.conflicted.len()
            ));
        }
        if !self.missing.is_empty() {
            parts.push(format!("{} are gone", self.missing.len()));
        }
        if self.trimmed > 0 {
            parts.push(format!("{} had their history shortened", self.trimmed));
        }
        (!parts.is_empty()).then(|| parts.join("; "))
    }
}

/// Puts `snapshot` back into `session`, and answers what could not be done.
///
/// Synchronous as far as the first frame goes; the rest arrives behind it.
pub fn apply(session: &Session, snapshot: &Snapshot, views: &mut Views) -> Report {
    let mut report = Report::default();
    let workspace = session.workspace();
    let directory = session
        .state()
        .map(|state| store::directory_for(&state, session.project().root()));

    // Which splits show which buffers, so the ones on screen can be read now and the rest later.
    let showing: Vec<u32> = snapshot
        .windows
        .iter()
        .filter_map(|window| window.current)
        .collect();

    // ---- Stage one: the buffers, as placeholders with real paths ------------------------------
    let mut made: Vec<Option<BufferId>> = Vec::with_capacity(snapshot.buffers.len());
    for (at, buffer) in snapshot.buffers.iter().enumerate() {
        let id = match buffer.kind {
            BufferSort::Text => {
                let visible = showing.contains(&(at as u32));
                Some(open_text(
                    session,
                    snapshot,
                    at,
                    visible,
                    directory.as_deref(),
                    &mut report,
                ))
            }
            BufferSort::Terminal => buffer.terminal.as_ref().and_then(|spec| {
                session.terminals().restore(
                    &crate::terminals::Program {
                        argv: spec.argv.clone(),
                        directory: spec.directory.clone(),
                    },
                    spec.listed,
                    spec.float.as_deref(),
                )
            }),
            BufferSort::Settings => Some(workspace.open_panel(BufferKind::Settings)),
            BufferSort::Git => Some(workspace.open_panel(BufferKind::Git)),
            // Something a later release writes. The buffer is left out rather than guessed at.
            BufferSort::Unknown => None,
        };
        made.push(id);
    }

    let live: Vec<BufferId> = made.iter().flatten().copied().collect();
    report.files = live.len();

    // ---- Stage two: the splits --------------------------------------------------------------
    let windows: Vec<Restored> = snapshot
        .windows
        .iter()
        .map(|window| Restored {
            current: window
                .current
                .and_then(|at| made.get(at as usize).copied().flatten())
                .and_then(|id| live.iter().position(|held| *held == id)),
            font_step: window.font_step,
        })
        .collect();

    let layout = &snapshot.layout;
    let made_windows = workspace.restore_layout(
        &windows,
        |ids| rebuild(layout, ids),
        &live,
        snapshot.focused as usize,
    );

    // The buffer line's order, and the buffer the workspace was made holding.
    let ordered: Vec<BufferId> = snapshot
        .order
        .iter()
        .filter_map(|at| made.get(*at as usize).copied().flatten())
        .collect();
    workspace.restore_order(&ordered);
    workspace.drop_scratch();
    workspace.set_alternate(
        snapshot
            .alternate
            .and_then(|at| made.get(at as usize).copied().flatten()),
    );
    workspace.restore_recent(snapshot.recent.clone());

    // ---- Stage three: where every editor was looking ----------------------------------------
    views.clear();
    for view in &snapshot.views {
        let Some(window) = made_windows.get(view.window as usize).copied() else {
            continue;
        };
        let Some(buffer) = made.get(view.buffer as usize).copied().flatten() else {
            continue;
        };
        views.insert((window, buffer), view.clone());
    }

    // ---- Stage four: the tree, vim's memory, and the command line ----------------------------
    restore_tree(session, snapshot);
    restore_vim(session, snapshot, &made);
    session
        .cmdline()
        .restore_history(snapshot.cmdline.history.clone());

    report
}

/// Opens one text buffer, reading its text now when it is one that will be on screen.
fn open_text(
    session: &Session,
    snapshot: &Snapshot,
    at: usize,
    visible: bool,
    directory: Option<&std::path::Path>,
    report: &mut Report,
) -> BufferId {
    let buffer = &snapshot.buffers[at];
    let workspace = session.workspace();

    // A buffer nobody is looking at opens as a placeholder with the right path, so the buffer
    // line and the pickers are correct on the first frame, and fills in behind.
    if !visible {
        let id = workspace.open_document(buffer.path.clone(), zgui_editor::Document::new(""));
        fill_in(session, snapshot, at, id, directory.map(PathBuf::from));
        return id;
    }

    let read = read_now(buffer, directory);
    match read.outcome {
        Outcome::Conflicted => {
            if let Some(path) = buffer.path.clone() {
                report.conflicted.push(path);
            }
        }
        Outcome::Missing => {
            if let Some(path) = buffer.path.clone() {
                report.missing.push(path);
            }
        }
        Outcome::Fine => {}
    }
    if read.trimmed {
        report.trimmed += 1;
    }

    let dirty = read.dirty;
    let on_disk = read.on_disk.unwrap_or_default();
    let id = workspace.open_document(buffer.path.clone(), read.document);
    if dirty && let Some(entry) = workspace.buffer_untracked(id) {
        // The fingerprint of what is on *disk*, so the dirty mark is about the text and not about
        // a revision this run has never seen. The file was read once, above.
        entry
            .saved_text
            .set(crate::workspace::buffer::Fingerprint::of(
                &ropey::Rope::from_str(&on_disk),
            ));
        entry.refresh_dirty();
    }
    id
}

/// One buffer, read back.
struct Read {
    document: zgui_editor::Document,
    dirty: bool,
    trimmed: bool,
    outcome: Outcome,
    /// What is on disk right now, when there is a file and it could be read.
    ///
    /// Carried out rather than read again: the caller needs it to work out whether the buffer is
    /// dirty, and reading the same file twice is the one cost here nobody chose.
    on_disk: Option<String>,
}

/// What reading one buffer back came to.
enum Outcome {
    /// It came back exactly.
    Fine,
    /// The file changed while the editor was closed, and the unsaved text was kept back.
    Conflicted,
    /// The file is not there any more.
    Missing,
}

/// Reads one buffer's text and history, deciding what the file on disk means for it.
///
/// | stored | on disk now | what happens |
/// |---|---|---|
/// | matches, clean | same | text from disk, history restored |
/// | matches, dirty | same | text and history from the blob, dirty |
/// | differs, clean | changed | from disk, history dropped, silent |
/// | differs, dirty | changed | from disk, history dropped, the blob kept back |
/// | never existed, dirty | still absent | restored: a file not written yet is a real buffer |
fn read_now(
    buffer: &crate::session::schema::BufferSnapshot,
    directory: Option<&std::path::Path>,
) -> Read {
    let blob = directory
        .zip(buffer.content.as_ref())
        .and_then(|(directory, held)| store::read_blob(directory, held));
    let stored = buffer.disk.unwrap_or_default();
    let now = buffer
        .path
        .as_deref()
        .map_or_else(DiskStamp::default, crate::session::capture::stamp);

    let on_disk = buffer
        .path
        .as_deref()
        .and_then(|path| std::fs::read_to_string(path).ok());

    let Some(blob) = blob else {
        return Read {
            document: zgui_editor::Document::new(on_disk.as_deref().unwrap_or("")),
            dirty: false,
            trimmed: false,
            outcome: if buffer.path.is_some() && !now.exists {
                Outcome::Missing
            } else {
                Outcome::Fine
            },
            on_disk,
        };
    };

    let history = zgui_editor::History::from_parts(blob.history.undo, blob.history.redo);
    let same = stored.matches(&now);

    match (same, blob.dirty, blob.text) {
        // Nothing moved. The text is whatever it was, and the history comes with it.
        (true, dirty, text) => {
            let text = text.or_else(|| on_disk.clone()).unwrap_or_default();
            Read {
                document: zgui_editor::Document::restore(&text, history),
                dirty,
                trimmed: blob.trimmed,
                outcome: Outcome::Fine,
                on_disk,
            }
        }
        // A file that never existed and still does not. A buffer for one not written yet is real.
        (false, true, Some(text)) if !stored.exists && !now.exists => Read {
            document: zgui_editor::Document::restore(&text, history),
            dirty: true,
            trimmed: blob.trimmed,
            outcome: Outcome::Fine,
            on_disk,
        },
        // It changed underneath. Never clobber and never resurrect: what is on disk wins, the
        // history goes with the text it described, and the unsaved text is reported.
        (false, dirty, _) => Read {
            document: zgui_editor::Document::new(on_disk.as_deref().unwrap_or("")),
            dirty: false,
            trimmed: false,
            outcome: if dirty {
                Outcome::Conflicted
            } else if buffer.path.is_some() && !now.exists {
                Outcome::Missing
            } else {
                Outcome::Fine
            },
            on_disk,
        },
    }
}

/// Reads one buffer's text on a worker and puts it into the placeholder that is holding its place.
fn fill_in(
    session: &Session,
    snapshot: &Snapshot,
    at: usize,
    id: BufferId,
    directory: Option<PathBuf>,
) {
    let buffer = snapshot.buffers[at].clone();
    let workspace = session.workspace().clone();

    zdt_view::detached(async move {
        let read = zgui::task::blocking(move || {
            let read = read_now(&buffer, directory.as_deref());
            (buffer.path.clone(), read.document.text(), read.dirty)
        })
        .await;

        let (path, text, dirty) = read;
        // Replacing a document is only safe while no view has mounted it, which holds by
        // construction: only the buffers that were *not* on screen come through here, and they
        // are in no split's mounted list.
        let document = zgui_editor::Document::new(&text);
        let saved = crate::workspace::buffer::Fingerprint::of(&document.rope());
        let entry = crate::workspace::Buffer::restored(id, path, document, saved);
        if dirty {
            entry.dirty.set(true);
        }
        workspace.replace_buffer(entry);
    });
}

/// The layout, over the split identities that were just made.
fn rebuild(node: &LayoutNode, windows: &[WindowId]) -> Option<Layout> {
    if let Some(at) = node.window {
        return windows.get(at as usize).copied().map(Layout::Leaf);
    }
    // A node that names nothing and holds nothing is one a later release invented. It is dropped,
    // and its space goes to whatever was beside it.
    let children: Vec<(Layout, f64)> = node
        .children
        .iter()
        .filter_map(|LayoutChild { node, share }| Some((rebuild(node, windows)?, *share)))
        .collect();
    match children.len() {
        0 => None,
        // A division with one child left is not a division.
        1 => Some(children.into_iter().next().expect("one").0),
        _ => {
            // Renormalised, so a file that lost a child cannot leave a division adding to 3.
            let total: f64 = children.iter().map(|(_, share)| share).sum();
            let children = if total > 0.0 {
                children
                    .into_iter()
                    .map(|(node, share)| (node, share / total * 100.0))
                    .collect()
            } else {
                let each = 100.0 / children.len() as f64;
                children.into_iter().map(|(node, _)| (node, each)).collect()
            };
            Some(Layout::Split {
                axis: match node.axis {
                    SplitAxis::Vertical => Axis::Vertical,
                    SplitAxis::Horizontal | SplitAxis::Unknown => Axis::Horizontal,
                },
                children,
            })
        }
    }
}

/// The file tree: what was open, and which row the caret was on.
fn restore_tree(session: &Session, snapshot: &Snapshot) {
    let explorer = session.explorer();
    if let Some(open) = snapshot.tree.open
        && open != explorer.is_open_untracked()
    {
        explorer.toggle();
    }
    explorer.restore_session(
        snapshot.tree.expanded.clone(),
        snapshot.tree.at.clone(),
        snapshot.tree.marked.clone(),
    );
}

/// Vim's memory, with every place pointed back at the buffer it was in.
fn restore_vim(session: &Session, snapshot: &Snapshot, made: &[Option<BufferId>]) {
    let owner = |place: &PlaceSnapshot| {
        let id = place
            .buffer
            .and_then(|at| made.get(at as usize).copied().flatten());
        zdt_vim::Place {
            owner: id.map_or_else(zdt_vim::Owner::default, |id| {
                zdt_vim::Owner(id.data().as_ffi())
            }),
            byte: place.byte as usize,
        }
    };

    session.vim().restore_memory(
        snapshot
            .vim
            .registers
            .iter()
            .map(|register| {
                (
                    register.name.clone(),
                    register.text.clone(),
                    register.linewise,
                )
            })
            .collect(),
        snapshot
            .vim
            .marks
            .iter()
            .filter_map(|mark| Some((mark.name.chars().next()?, owner(&mark.place))))
            .collect(),
        snapshot.vim.jumps.iter().map(owner).collect(),
        snapshot.vim.jump_at as usize,
    );
}

/// Puts every editor back where it was looking, as each one mounts.
///
/// A [`RenderEffect`](zgui::reactive::RenderEffect) on the mount revision, and never a timer: an
/// editor for a file that is still being read mounts several frames after the restore, and a
/// timer that has already fired has nothing to say to it.
pub fn follow_mounts(
    session: &Session,
    waiting: std::rc::Rc<std::cell::RefCell<Views>>,
) -> zgui::reactive::RenderEffect<()> {
    let workspace = session.workspace().clone();
    zgui::reactive::RenderEffect::new(move |_| {
        // Read first: an editor mounting is the one thing that changes without anything else
        // changing, and it is what has to wake this.
        let _ = workspace.mounted_revision();
        // What can be applied is decided while the map is borrowed, and applied after it is not:
        // putting a view back tells the editor, and the editor records where it is looking
        // through this same map.
        let ready: Vec<(zgui_editor::EditorHandle, ViewSnapshot)> = {
            let owed = waiting.borrow();
            if owed.is_empty() {
                return;
            }
            owed.iter()
                .filter_map(|((window, buffer), view)| {
                    Some((workspace.handle_for(*window, *buffer)?, view.clone()))
                })
                .collect()
        };
        if ready.is_empty() {
            return;
        }
        waiting
            .borrow_mut()
            .retain(|(window, buffer), _| workspace.handle_for(*window, *buffer).is_none());
        for (handle, view) in &ready {
            place(handle, view);
        }
    })
}

/// Puts one editor's carets and view back.
fn place(handle: &zgui_editor::EditorHandle, view: &ViewSnapshot) {
    let selections: Vec<zgui_editor::Selection> = view
        .selections
        .iter()
        .map(|SelectionSnapshot { anchor, head }| {
            zgui_editor::Selection::new(*anchor as usize, *head as usize)
        })
        .collect();
    if !selections.is_empty() {
        // The selections first, then the view. Setting selections answers with no scroll of its
        // own, so the order is exact and the view lands where it was rather than wherever the
        // caret would have dragged it.
        handle.command(zgui_editor::Command::SetSelections {
            selections,
            primary: view.primary as usize,
        });
    }
    handle.command(zgui_editor::Command::Scroll(
        zgui_editor::ScrollCmd::ToExact {
            line: view.top_line,
            x_px: view.x_px,
        },
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::schema::WindowSnapshot;

    fn ready() {
        zgui::reactive::install().expect("the reactive runtime installs");
    }

    /// Some split identities, which is all the layout rebuild needs.
    fn windows(count: usize) -> Vec<WindowId> {
        let mut map: slotmap::SlotMap<WindowId, ()> = slotmap::SlotMap::with_key();
        (0..count).map(|_| map.insert(())).collect()
    }

    #[test]
    fn a_leaf_names_the_split_at_its_index() {
        ready();
        let ids = windows(2);
        let layout = rebuild(&LayoutNode::leaf(1), &ids).expect("it rebuilds");
        assert_eq!(layout, Layout::Leaf(ids[1]));
    }

    #[test]
    fn a_leaf_naming_a_split_that_is_not_there_is_dropped() {
        ready();
        assert!(rebuild(&LayoutNode::leaf(7), &windows(2)).is_none());
    }

    #[test]
    fn a_division_keeps_its_shares() {
        ready();
        let ids = windows(2);
        let node = LayoutNode {
            axis: SplitAxis::Vertical,
            children: vec![
                LayoutChild {
                    node: LayoutNode::leaf(0),
                    share: 70.0,
                },
                LayoutChild {
                    node: LayoutNode::leaf(1),
                    share: 30.0,
                },
            ],
            ..LayoutNode::default()
        };
        let Some(Layout::Split { axis, children }) = rebuild(&node, &ids) else {
            panic!("a division");
        };
        assert_eq!(axis, Axis::Vertical);
        assert!((children[0].1 - 70.0).abs() < 0.001);
        assert!((children[1].1 - 30.0).abs() < 0.001);
    }

    #[test]
    fn a_division_that_lost_a_child_is_renormalised() {
        // Otherwise a file naming a split that is gone leaves a division adding to 30.
        ready();
        let ids = windows(1);
        let node = LayoutNode {
            axis: SplitAxis::Horizontal,
            children: vec![
                LayoutChild {
                    node: LayoutNode::leaf(0),
                    share: 30.0,
                },
                LayoutChild {
                    node: LayoutNode::leaf(9),
                    share: 70.0,
                },
                LayoutChild {
                    node: LayoutNode::leaf(0),
                    share: 30.0,
                },
            ],
            ..LayoutNode::default()
        };
        let Some(Layout::Split { children, .. }) = rebuild(&node, &ids) else {
            panic!("a division");
        };
        let total: f64 = children.iter().map(|(_, share)| share).sum();
        assert!((total - 100.0).abs() < 0.001, "the shares add to a hundred");
    }

    #[test]
    fn a_division_with_one_child_left_is_not_a_division() {
        ready();
        let ids = windows(1);
        let node = LayoutNode {
            children: vec![
                LayoutChild {
                    node: LayoutNode::leaf(0),
                    share: 50.0,
                },
                LayoutChild {
                    node: LayoutNode::leaf(9),
                    share: 50.0,
                },
            ],
            ..LayoutNode::default()
        };
        assert_eq!(rebuild(&node, &ids), Some(Layout::Leaf(ids[0])));
    }

    #[test]
    fn a_node_a_later_release_invented_is_dropped() {
        ready();
        assert!(rebuild(&LayoutNode::default(), &windows(2)).is_none());
    }

    #[test]
    fn a_report_that_lost_nothing_says_nothing() {
        assert_eq!(Report::default().say(), None);
        let report = Report {
            files: 3,
            ..Report::default()
        };
        assert_eq!(report.say(), None);
    }

    #[test]
    fn a_report_says_what_it_could_not_do() {
        let report = Report {
            files: 3,
            conflicted: vec![PathBuf::from("/one")],
            missing: vec![PathBuf::from("/two")],
            trimmed: 2,
        };
        let said = report.say().expect("there is something to say");
        assert!(said.contains("changed on disk"));
        assert!(said.contains("gone"));
        assert!(said.contains("shortened"));
    }

    #[test]
    fn a_window_snapshot_with_no_buffer_is_a_real_state() {
        // A split showing nothing is something somebody can arrive at with `<C-w>` and `bd`.
        let window = WindowSnapshot::default();
        assert_eq!(window.current, None);
    }
}
