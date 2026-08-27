//! What a session is written down as.
//!
//! MessagePack with named fields, encoded with `rmp_serde::to_vec_named`. Binary, so it is fast
//! and small; named, so it describes itself and a field added in a later release is invisible to
//! an earlier one.
//!
//! # The compatibility rule
//!
//! Every struct here is `#[serde(default)]` and **never** `deny_unknown_fields` — the opposite of
//! the convention the configuration uses, and deliberately so. Configuration is written by hand
//! and a typo in it is a mistake worth reporting; this is written by the editor and read by
//! whichever version happens to be installed.
//!
//!   * **A field added.** An older zdt ignores it; a newer one reading an older file gets the
//!     default. Nothing breaks either way, and no code is needed.
//!   * **A field removed.** The same, in reverse. Do not reuse the name for anything else — that
//!     is the one rule.
//!   * **A type changed.** Not compatible. Add a new field with a new name, read both for a
//!     release, then drop the old one. `#[serde(alias = "…")]` covers a rename.
//!   * **A variant added.** Every closed vocabulary here has a `#[serde(other)] Unknown`, so a
//!     value a later release writes decodes to something this one can decide about.
//!
//! [`FORMAT`] is a tripwire and not a version to branch on: it moves only if the container stops
//! being MessagePack, at which point the file is ignored whole.
//!
//! # Why the layout is a struct and not an enum
//!
//! A leaf has a `window` and no `children`; a split has `children`. A node that is neither — what
//! a variant this release has never heard of decodes to — is dropped, and its space goes to
//! whatever was beside it. An enum would fail the whole decode instead.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The container this release writes.
pub const FORMAT: u32 = 1;

/// One saved session.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    /// A tripwire, and not a version to branch on.
    pub format: u32,
    /// Which zdt wrote it, for the report when something does not read.
    pub wrote: String,
    /// Counts up once per write, so a second editor writing the same session can be noticed.
    pub generation: u64,
    /// When, in milliseconds since the epoch.
    pub written_at_ms: u64,
    /// The directory this is the session for, canonical.
    #[serde(with = "os_path")]
    pub root: PathBuf,

    /// Every open buffer. Its place in this list is the identity everything else uses.
    pub buffers: Vec<BufferSnapshot>,
    /// The buffer line's order, by place in `buffers`.
    pub order: Vec<u32>,
    /// Every split. Its place in this list is what the layout names.
    pub windows: Vec<WindowSnapshot>,
    /// How the splits are arranged.
    pub layout: LayoutNode,
    /// Which split had the keyboard.
    pub focused: u32,
    /// What `<Leader>bp` would have gone back to.
    pub alternate: Option<u32>,
    /// Where each editor was looking.
    pub views: Vec<ViewSnapshot>,

    pub tree: TreeSnapshot,
    pub vim: VimSnapshot,
    pub cmdline: CmdlineSnapshot,
    pub agent: AgentSnapshot,
    /// Every file opened, most recent first, for the picker.
    #[serde(with = "os_paths")]
    pub recent: Vec<PathBuf>,
}

/// One node of the arrangement.
///
/// A leaf has `window` and no `children`; a split has `children`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutNode {
    /// Which split, by place in [`Snapshot::windows`]. Absent on a division.
    pub window: Option<u32>,
    /// Which way a division divides. Meaningless on a leaf.
    pub axis: SplitAxis,
    /// What is in a division, each with its share of it.
    pub children: Vec<LayoutChild>,
}

impl LayoutNode {
    /// A leaf naming the split at `window`.
    #[must_use]
    pub fn leaf(window: u32) -> Self {
        Self {
            window: Some(window),
            ..Self::default()
        }
    }

    /// Whether this node says nothing at all, which is what an unknown one decodes to.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.window.is_none() && self.children.is_empty()
    }
}

/// One child of a division, and how much of it it takes.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutChild {
    pub node: LayoutNode,
    /// Its share of the division, as a percentage.
    pub share: f64,
}

/// Which way a division divides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitAxis {
    /// Side by side, the way `:vsplit` divides.
    #[default]
    Horizontal,
    /// One above the other, the way `:split` does.
    Vertical,
    /// Something a later zdt writes. Read as side by side.
    #[serde(other)]
    Unknown,
}

/// One split.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowSnapshot {
    /// Which buffer it was showing. A split showing nothing is a real state.
    pub current: Option<u32>,
    /// How much larger its text was than the setting says.
    pub font_step: i32,
    /// Which buffers it showed in their rich form.
    pub rich: Vec<u32>,
}

/// Where one editor was looking: one per split-and-buffer that had an editor mounted.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewSnapshot {
    pub window: u32,
    pub buffer: u32,
    /// The selections, in document order.
    pub selections: Vec<SelectionSnapshot>,
    /// Which of them was primary.
    pub primary: u32,
    /// The line at the top of the view, fractionally.
    pub top_line: f64,
    /// How far the text was scrolled left, in device pixels.
    pub x_px: f64,
}

/// One selection: two byte offsets.
///
/// The affinity and the goal column are left out. A goal column is the state of a run of `j`,
/// which no longer exists, and an affinity is decided again the moment the caret moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SelectionSnapshot {
    pub anchor: u64,
    pub head: u64,
}

/// What kind of thing a buffer holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BufferSort {
    #[default]
    Text,
    Terminal,
    Settings,
    Git,
    /// A kind a later zdt has and this one does not. The buffer is left out.
    #[serde(other)]
    Unknown,
}

/// One open buffer.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BufferSnapshot {
    pub kind: BufferSort,
    /// Where it came from, absolute, when it came from anywhere.
    #[serde(with = "os_path_option")]
    pub path: Option<PathBuf>,
    /// How it is spelled on disk: `utf-8`, `utf-8-bom`, `utf-16le`, `utf-16be`.
    pub encoding: String,
    /// What it breaks its lines with: `lf` or `crlf`.
    pub line_ending: String,
    /// Whether bytes had to be replaced to read it. Such a buffer must not be written back.
    pub lossy: bool,
    /// Where its text and its history are, when they were worth writing.
    pub content: Option<ContentRef>,
    /// What the file looked like when this was taken.
    pub disk: Option<DiskStamp>,
    /// The program, when this is a terminal.
    pub terminal: Option<TerminalSpec>,
}

/// Where one buffer's heavy half is.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ContentRef {
    /// Relative to the session directory: `buffers/0007.msgpack`.
    pub file: String,
    pub bytes: u64,
    /// Of the blob's bytes, so a truncated one is caught rather than decoded.
    pub hash: u64,
}

/// What the file on disk was, so a restore can tell whether it moved underneath.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiskStamp {
    /// Whether the file was there at all. A buffer for a file not written yet is a real state.
    pub exists: bool,
    pub len: u64,
    /// Milliseconds since the epoch. Advisory: it can go backwards over a network share.
    pub mtime_ms: i64,
    /// Of the file's raw bytes. The one that decides.
    pub hash: u64,
}

impl DiskStamp {
    /// Whether `other` describes the same file contents.
    ///
    /// The length short-circuits, and the hash decides. The time is never trusted alone: a
    /// network share can move it backwards and a checkout can leave it untouched.
    #[must_use]
    pub fn matches(&self, other: &Self) -> bool {
        self.exists == other.exists && self.len == other.len && self.hash == other.hash
    }
}

/// A terminal's identity: what to run, where, and how it was reached.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalSpec {
    /// The program and its arguments. Empty means the login shell.
    pub argv: Vec<String>,
    #[serde(with = "os_path_option")]
    pub directory: Option<PathBuf>,
    /// Whether it was on the buffer line.
    pub listed: bool,
    /// Which float it was, when it was one: "", "lazygit", "python".
    pub float: Option<String>,
}

/// One buffer's text and undo history.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BufferContent {
    pub format: u32,
    /// The text as the editor holds it: UTF-8, with `\n` breaks.
    ///
    /// Empty when the buffer matched the file on disk, which is the common case: the text is
    /// already there and writing it twice would double the size of a session for nothing.
    pub text: Option<String>,
    /// Whether it differed from the file when this was taken.
    pub dirty: bool,
    pub history: HistorySnapshot,
    /// Whether the history was cut to fit. Said out loud on restore.
    pub trimmed: bool,
}

/// The undone and the redoable.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HistorySnapshot {
    /// Oldest first, the way the stack holds them.
    pub undo: Vec<zgui_editor::Step>,
    pub redo: Vec<zgui_editor::Step>,
}

/// The file tree.
///
/// Only what a session can own. `tree.width`, `tree.hidden`, `tree.ignored` and `tree.follow` are
/// written into `config.toml` by the interface itself — dragging the divider calls
/// `Settings::edit` — so they are settings, and a session that carried them would fight the file
/// the person edits.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TreeSnapshot {
    /// Whether the panel was open, when that differs from what `tree.open` says.
    ///
    /// `None` means "whatever the settings say", which is what a session that never touched the
    /// panel writes. So changing `tree.open` by hand still takes effect.
    pub open: Option<bool>,
    /// Which directories were open, parents before children.
    #[serde(with = "os_paths")]
    pub expanded: Vec<PathBuf>,
    /// Which row the caret was on, by path.
    ///
    /// Never by index: a directory opening above it moves every index below.
    #[serde(with = "os_path_option")]
    pub at: Option<PathBuf>,
    #[serde(with = "os_paths")]
    pub marked: Vec<PathBuf>,
}

/// The agent surface, as this session's window last showed it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentSnapshot {
    /// Which face the window showed.
    pub face: FaceSort,
    /// Which thread the chat showed, by the daemon's own id.
    pub thread: Option<i64>,
    /// Whether the sidebar was on screen. Nothing on snapshots from before it was written, and
    /// the configuration decides then.
    pub side_open: Option<bool>,
}

/// Which face a window shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaceSort {
    /// The editor: the tree, the splits, the buffer line.
    #[default]
    Editor,
    /// The chat view of the selected thread.
    Agent,
    /// A face a later zdt has and this one does not. Read as the editor.
    #[serde(other)]
    Unknown,
}

/// Vim's memory.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VimSnapshot {
    pub registers: Vec<RegisterSnapshot>,
    pub marks: Vec<MarkSnapshot>,
    /// Where the caret has been, oldest first.
    pub jumps: Vec<PlaceSnapshot>,
    /// How far back through them `<C-o>` had walked.
    pub jump_at: u32,
}

/// One register. `name` is empty for the unnamed one.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RegisterSnapshot {
    pub name: String,
    pub text: String,
    pub linewise: bool,
}

/// A place in a buffer.
///
/// The buffer, and not just the offset. A bare byte offset from a previous run names a place in a
/// file that something else may have edited, and `'a` landing three functions away is worse than
/// `'a` doing nothing.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaceSnapshot {
    pub buffer: Option<u32>,
    pub byte: u64,
}

/// One mark, and where it is.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MarkSnapshot {
    pub name: String,
    pub place: PlaceSnapshot,
}

/// The command line.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CmdlineSnapshot {
    /// What was typed before, most recent last, capped as the live list is.
    pub history: Vec<String>,
}

/// Paths, as bytes on unix and text everywhere else.
///
/// `PathBuf`'s own `Serialize` goes through `to_str` and **fails** on a path that is not UTF-8,
/// which is a real thing on Linux. One buffer with such a path would take the whole session with
/// it.
pub mod os_path {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::path::PathBuf;

    /// Writes `path`.
    ///
    /// # Errors
    ///
    /// Whatever the encoder says.
    #[allow(clippy::ptr_arg)]
    pub fn serialize<S: Serializer>(path: &PathBuf, out: S) -> Result<S::Ok, S::Error> {
        // `&PathBuf` because this is what `#[serde(with)]` hands a `PathBuf` field.
        super::bytes_of(path).serialize(out)
    }

    /// Reads one.
    ///
    /// # Errors
    ///
    /// Whatever the decoder says.
    pub fn deserialize<'de, D: Deserializer<'de>>(input: D) -> Result<PathBuf, D::Error> {
        Ok(super::path_of(&Vec::<u8>::deserialize(input)?))
    }
}

/// The same, for a path that may not be there.
pub mod os_path_option {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::path::PathBuf;

    /// Writes `path`, or nothing.
    ///
    /// # Errors
    ///
    /// Whatever the encoder says.
    pub fn serialize<S: Serializer>(path: &Option<PathBuf>, out: S) -> Result<S::Ok, S::Error> {
        path.as_deref().map(super::bytes_of).serialize(out)
    }

    /// Reads one.
    ///
    /// # Errors
    ///
    /// Whatever the decoder says.
    pub fn deserialize<'de, D: Deserializer<'de>>(input: D) -> Result<Option<PathBuf>, D::Error> {
        Ok(Option::<Vec<u8>>::deserialize(input)?
            .as_deref()
            .map(super::path_of))
    }
}

/// The same, for a list of them.
pub mod os_paths {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::path::PathBuf;

    /// Writes them.
    ///
    /// # Errors
    ///
    /// Whatever the encoder says.
    pub fn serialize<S: Serializer>(paths: &[PathBuf], out: S) -> Result<S::Ok, S::Error> {
        paths
            .iter()
            .map(|path| super::bytes_of(path))
            .collect::<Vec<_>>()
            .serialize(out)
    }

    /// Reads them.
    ///
    /// # Errors
    ///
    /// Whatever the decoder says.
    pub fn deserialize<'de, D: Deserializer<'de>>(input: D) -> Result<Vec<PathBuf>, D::Error> {
        Ok(Vec::<Vec<u8>>::deserialize(input)?
            .iter()
            .map(|bytes| super::path_of(bytes))
            .collect())
    }
}

/// A path as bytes.
#[cfg(unix)]
fn bytes_of(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

/// A path as bytes. Lossy off unix, where a path is text to begin with.
#[cfg(not(unix))]
fn bytes_of(path: &Path) -> Vec<u8> {
    path.to_string_lossy().as_bytes().to_vec()
}

/// Bytes as a path.
#[cfg(unix)]
fn path_of(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

/// Bytes as a path.
#[cfg(not(unix))]
fn path_of(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a session round-trips through.
    fn round<T: Serialize + serde::de::DeserializeOwned>(value: &T) -> T {
        let bytes = rmp_serde::to_vec_named(value).expect("it encodes");
        rmp_serde::from_slice(&bytes).expect("it decodes")
    }

    #[test]
    fn a_snapshot_round_trips() {
        let snapshot = Snapshot {
            format: FORMAT,
            root: PathBuf::from("/home/someone/work"),
            order: vec![1, 0],
            focused: 1,
            ..Snapshot::default()
        };
        let back = round(&snapshot);
        assert_eq!(back.root, snapshot.root);
        assert_eq!(back.order, snapshot.order);
        assert_eq!(back.focused, 1);
    }

    #[test]
    fn a_field_a_later_release_added_is_ignored() {
        // The whole reason the fields are named and nothing denies unknown ones.
        #[derive(Serialize)]
        struct Later {
            format: u32,
            order: Vec<u32>,
            /// Something this release has never heard of.
            colour_scheme_per_split: String,
        }
        let bytes = rmp_serde::to_vec_named(&Later {
            format: FORMAT,
            order: vec![3],
            colour_scheme_per_split: "moonlight".to_owned(),
        })
        .expect("it encodes");

        let back: Snapshot = rmp_serde::from_slice(&bytes).expect("it decodes anyway");
        assert_eq!(back.order, vec![3]);
    }

    #[test]
    fn a_field_this_release_expects_and_an_older_file_lacks_is_a_default() {
        #[derive(Serialize)]
        struct Older {
            format: u32,
        }
        let bytes = rmp_serde::to_vec_named(&Older { format: FORMAT }).expect("it encodes");
        let back: Snapshot = rmp_serde::from_slice(&bytes).expect("it decodes");
        assert_eq!(back.format, FORMAT);
        assert!(back.buffers.is_empty());
        assert_eq!(back.focused, 0);
    }

    #[test]
    fn a_window_snapshot_keeps_its_rich_buffers() {
        let snapshot = WindowSnapshot {
            current: Some(0),
            font_step: 2,
            rich: vec![0, 3],
        };
        let back = round(&snapshot);
        assert_eq!(back.rich, vec![0, 3]);

        // A file an older release wrote has no such field, and reads as none.
        #[derive(Serialize)]
        struct Older {
            current: Option<u32>,
        }
        let bytes = rmp_serde::to_vec_named(&Older { current: Some(1) }).expect("it encodes");
        let back: WindowSnapshot = rmp_serde::from_slice(&bytes).expect("it decodes");
        assert!(back.rich.is_empty());
    }

    #[test]
    fn a_variant_a_later_release_added_reads_as_unknown() {
        #[derive(Serialize)]
        struct Later {
            kind: &'static str,
        }
        let bytes = rmp_serde::to_vec_named(&Later { kind: "notebook" }).expect("it encodes");
        let back: BufferSnapshot = rmp_serde::from_slice(&bytes).expect("it decodes");
        assert_eq!(back.kind, BufferSort::Unknown);
    }

    #[test]
    fn a_layout_node_that_says_nothing_is_recognised() {
        // Which is what a division shape a later release invented decodes to.
        assert!(LayoutNode::default().is_empty());
        assert!(!LayoutNode::leaf(0).is_empty());
    }

    #[test]
    fn a_layout_tree_round_trips() {
        let layout = LayoutNode {
            axis: SplitAxis::Vertical,
            children: vec![
                LayoutChild {
                    node: LayoutNode::leaf(0),
                    share: 60.0,
                },
                LayoutChild {
                    node: LayoutNode::leaf(1),
                    share: 40.0,
                },
            ],
            ..LayoutNode::default()
        };
        let back = round(&layout);
        assert_eq!(back.axis, SplitAxis::Vertical);
        assert_eq!(back.children.len(), 2);
        assert!((back.children[0].share - 60.0).abs() < f64::EPSILON);
        assert_eq!(back.children[1].node.window, Some(1));
    }

    #[test]
    #[cfg(unix)]
    fn a_path_that_is_not_text_still_round_trips() {
        // `PathBuf`'s own serialiser fails on this, and one such buffer would take the whole
        // session with it.
        use std::os::unix::ffi::OsStrExt;
        let awkward = PathBuf::from(std::ffi::OsStr::from_bytes(b"/tmp/\xff\xfe"));
        let snapshot = Snapshot {
            root: awkward.clone(),
            ..Snapshot::default()
        };
        assert_eq!(round(&snapshot).root, awkward);
    }

    #[test]
    fn a_stamp_compares_by_contents_and_not_by_time() {
        let one = DiskStamp {
            exists: true,
            len: 12,
            mtime_ms: 1,
            hash: 99,
        };
        let later = DiskStamp {
            mtime_ms: 500,
            ..one
        };
        assert!(one.matches(&later), "a touched file is the same file");

        let changed = DiskStamp { hash: 100, ..one };
        assert!(!one.matches(&changed));
    }
}
