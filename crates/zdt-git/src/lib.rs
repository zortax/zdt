//! Reading and changing a git repository.
//!
//! Everything here is blocking and knows nothing about the interface — no signals, no components,
//! no `Rc`. That is what lets it be tested against a real repository in a temporary directory,
//! which is the only way to have any confidence in code that stages hunks.
//!
//! # Why `gix` and not the `git` command
//!
//! For reads, because the questions the panel asks cannot be answered in one process. A commit
//! graph is thirty thousand commits and their parents; asking `git show` once per commit is
//! thirty thousand process spawns, and asking `git log` for everything at once is a megabyte of
//! text to re-parse whenever anything moves.
//!
//! For writes, because staging one hunk of a file is not a command-line operation. What it
//! actually is: read the blob the index holds, apply the chosen hunks to *that* text, write the
//! result as a new blob, and point the index entry at it. Expressed through `git apply --cached`
//! it is a patch that has to be generated, escaped, and applied to a file that may have moved
//! underneath it; expressed against the object store it is four steps that either all happen or
//! none do.
//!
//! One thing still runs a process — see [`hunks`], whose whole job is to be the cheapest possible
//! answer to "has this file changed at all" for a gutter that asks per file per save.

pub mod commit;
pub mod diff;
pub mod graph;
pub mod hunks;
pub mod log;
pub mod refs;
pub mod repo;
pub mod stage;
pub mod status;

pub use crate::commit::commit;
pub use crate::diff::{DiffHunk, FileDiff, Line, LineKind};
pub use crate::graph::{Edge, Row};
pub use crate::hunks::{Change, Hunk, after, before, hunks, parse};
pub use crate::log::Commit;
pub use crate::refs::{Branch, Head, branches, head};
pub use crate::repo::{Error, Repo};
pub use crate::status::{Entry, State};
