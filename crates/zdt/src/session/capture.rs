//! The running editor, as one value that can be written down.
//!
//! Indices, and never slotmap keys: a `BufferId` means nothing after a restart, so a buffer is
//! named by its place in the buffer list and a split by its place in the window list.
//!
//! # What is deliberately left out
//!
//! `WindowState::mounted` beyond what is showing — a warm cache of ready editors, invisible to
//! the person using the editor, which refills itself. Terminal contents, which are a lie the
//! moment the program behind them is restarted. The language servers, which re-derive from the
//! restored buffer list. Diagnostics, git signs and syntax trees, all derived and all wrong
//! within seconds of being written. Anything mid-gesture: a pending count, a half-typed operator,
//! the picker's query. And the theme, the font size and the keymap, which are configuration.

use std::path::Path;

use rustc_hash::FxHashMap;
use slotmap::Key;
use zgui::prelude::*;

use crate::session::Session;
use crate::session::schema::{
    BufferSnapshot, BufferSort, CmdlineSnapshot, DiskStamp, LayoutChild, LayoutNode, MarkSnapshot,
    PlaceSnapshot, RegisterSnapshot, SelectionSnapshot, Snapshot, SplitAxis, TerminalSpec,
    TreeSnapshot, ViewSnapshot, VimSnapshot, WindowSnapshot,
};
use crate::workspace::{Axis, BufferId, BufferKind, Layout, WindowId, Workspace};

/// Where each editor was looking, kept for editors that have since gone away.
///
/// `MOUNTED_PER_WINDOW` evicts editors, and an evicted one's handle is gone long before the next
/// save. So this is the truth, refreshed from the live handles whenever a snapshot is taken and
/// written from a view's own cleanup otherwise.
pub type Views = FxHashMap<(WindowId, BufferId), ViewSnapshot>;

/// Everything about the running session, as one value.
///
/// `views` is the cache described above, and is only *read* here.
///
/// Nothing in this function asks an editor anything. A save can be owed at the moment a window is
/// being taken apart, and an editor whose scope is being disposed of answers a signal read by
/// panicking — in a destructor, which aborts the process rather than unwinding. So the caret and
/// the scroll come from the cache the editor filled while it was alive, and never from the editor
/// itself.
#[must_use]
pub fn capture(session: &Session, views: &Views, generation: u64) -> Snapshot {
    let workspace = session.workspace();
    let order = workspace.order_untracked();

    // The buffer's index is its identity everywhere else in the file, so this mapping is built
    // first and everything below reads it.
    let mut buffers: Vec<BufferSnapshot> = Vec::with_capacity(order.len());
    let mut index_of: FxHashMap<BufferId, u32> = FxHashMap::default();
    for id in &order {
        let Some(buffer) = workspace.buffer_untracked(*id) else {
            continue;
        };
        let Some(mut snapshot) = describe(&buffer) else {
            continue;
        };
        if snapshot.kind == BufferSort::Terminal {
            snapshot.terminal =
                session
                    .terminals()
                    .spec_for(*id)
                    .map(|(program, float)| TerminalSpec {
                        argv: program.argv,
                        directory: program.directory,
                        listed: true,
                        float,
                    });
        }
        index_of.insert(*id, buffers.len() as u32);
        buffers.push(snapshot);
    }

    let (windows, index_of_window) = describe_windows(workspace, &index_of);

    Snapshot {
        format: crate::session::schema::FORMAT,
        wrote: env!("CARGO_PKG_VERSION").to_owned(),
        generation,
        written_at_ms: zdt_core::state::now_ms(),
        root: session.project().root().to_path_buf(),
        order: order
            .iter()
            .filter_map(|id| index_of.get(id).copied())
            .collect(),
        buffers,
        layout: describe_layout(&workspace.layout_untracked(), &index_of_window),
        focused: index_of_window
            .get(&workspace.focused_untracked())
            .copied()
            .unwrap_or(0),
        alternate: workspace
            .alternate_untracked()
            .and_then(|id| index_of.get(&id).copied()),
        views: views
            .iter()
            .filter_map(|((window, buffer), view)| {
                Some(ViewSnapshot {
                    window: index_of_window.get(window).copied()?,
                    buffer: index_of.get(buffer).copied()?,
                    ..view.clone()
                })
            })
            .collect(),
        windows,
        tree: describe_tree(session),
        vim: describe_vim(session, &index_of),
        cmdline: CmdlineSnapshot {
            history: session.cmdline().history(),
        },
        agent: session.agent_view(),
        recent: workspace.recent(),
    }
}

/// One buffer, or nothing when it is not worth writing down.
fn describe(buffer: &crate::workspace::Buffer) -> Option<BufferSnapshot> {
    let kind = match &buffer.kind {
        BufferKind::Text { .. } => BufferSort::Text,
        BufferKind::Terminal { .. } => BufferSort::Terminal,
        BufferKind::Settings => BufferSort::Settings,
        BufferKind::Git => BufferSort::Git,
    };
    Some(BufferSnapshot {
        kind,
        path: buffer.path.clone(),
        encoding: buffer.encoding.label().to_owned(),
        line_ending: buffer.line_ending.label().to_owned(),
        lossy: buffer.lossy,
        // Filled in by the writer, which is the half that reads the disk and the blobs.
        content: None,
        disk: None,
        terminal: None,
    })
}

/// Every split, and which index each became.
fn describe_windows(
    workspace: &Workspace,
    buffers: &FxHashMap<BufferId, u32>,
) -> (Vec<WindowSnapshot>, FxHashMap<WindowId, u32>) {
    // The layout's own order, which is the order `<C-w>w` walks in and the order a session has
    // always been written down in.
    let order = workspace.layout_untracked().windows();
    let mut index_of: FxHashMap<WindowId, u32> = FxHashMap::default();
    let mut windows = Vec::with_capacity(order.len());

    for id in order {
        let Some(state) = workspace.window(id) else {
            continue;
        };
        index_of.insert(id, windows.len() as u32);
        windows.push(WindowSnapshot {
            current: state.current.and_then(|held| buffers.get(&held).copied()),
            font_step: state.font_step,
        });
    }
    (windows, index_of)
}

/// The arrangement, over indices.
fn describe_layout(layout: &Layout, windows: &FxHashMap<WindowId, u32>) -> LayoutNode {
    match layout {
        Layout::Leaf(id) => windows
            .get(id)
            .map_or_else(LayoutNode::default, |at| LayoutNode::leaf(*at)),
        Layout::Split { axis, children } => LayoutNode {
            window: None,
            axis: match axis {
                Axis::Horizontal => SplitAxis::Horizontal,
                Axis::Vertical => SplitAxis::Vertical,
            },
            children: children
                .iter()
                .map(|(node, share)| LayoutChild {
                    node: describe_layout(node, windows),
                    share: *share,
                })
                .collect(),
        },
    }
}

/// Where one editor is looking.
#[must_use]
pub fn look(handle: &zgui_editor::EditorHandle) -> ViewSnapshot {
    // Untracked: a snapshot is taken from a debounce timer, where there is no reactive context
    // to subscribe to, and reading tracked there is a warning about a dependency nobody wants.
    let scroll = handle.scroll_state().get_untracked();
    handle.query(|snapshot| ViewSnapshot {
        window: 0,
        buffer: 0,
        selections: snapshot
            .selections()
            .iter()
            .map(|selection| SelectionSnapshot {
                anchor: selection.anchor as u64,
                head: selection.head as u64,
            })
            .collect(),
        primary: snapshot.selections().primary_index() as u32,
        // Where the view is heading, and not where it has reached. A jump glides, so the moment
        // a scroll is reported the view has barely left the line it was on.
        top_line: scroll.target_line,
        x_px: scroll.x_px,
    })
}

/// The file tree.
fn describe_tree(session: &Session) -> TreeSnapshot {
    let explorer = session.explorer();
    let (expanded, at, marked) = explorer.session_state();
    TreeSnapshot {
        open: Some(explorer.is_open_untracked()),
        expanded,
        at,
        marked,
    }
}

/// Vim's memory: what is in the registers, where the marks are, and where the caret has been.
fn describe_vim(session: &Session, buffers: &FxHashMap<BufferId, u32>) -> VimSnapshot {
    let vim = session.vim();
    let place = |place: zdt_vim::Place| PlaceSnapshot {
        buffer: buffers
            .iter()
            .find(|(id, _)| id.data().as_ffi() == place.owner.0)
            .map(|(_, at)| *at),
        byte: place.byte as u64,
    };

    let (jumps, jump_at) = vim.jumps();
    VimSnapshot {
        registers: vim
            .registers_full()
            .into_iter()
            .map(|(name, text, linewise)| RegisterSnapshot {
                name,
                text,
                linewise,
            })
            .collect(),
        marks: vim
            .marks()
            .into_iter()
            .map(|(name, held)| MarkSnapshot {
                name: name.to_string(),
                place: place(held),
            })
            .collect(),
        jumps: jumps.into_iter().map(place).collect(),
        jump_at: jump_at as u32,
    }
}

/// What a file on disk looks like right now.
///
/// Blocking. Called on a worker.
#[must_use]
pub fn stamp(path: &Path) -> DiskStamp {
    let Ok(bytes) = std::fs::read(path) else {
        return DiskStamp::default();
    };
    let mtime_ms = std::fs::metadata(path)
        .and_then(|data| data.modified())
        .ok()
        .and_then(|when| when.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_millis() as i64);
    DiskStamp {
        exists: true,
        len: bytes.len() as u64,
        mtime_ms,
        // Of the raw bytes, and never of the decoded text: what is being asked is whether the
        // file changed, and a hash that is stable across releases is the only one worth writing.
        hash: zdt_core::state::stable_hash(&bytes),
    }
}
