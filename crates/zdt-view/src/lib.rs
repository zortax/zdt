//! The small pieces a zgui view is built from.
//!
//! What every region of an application needs and `zgui` does not carry: a way to make two branches
//! of a choice one type, the exit state a presence is in, work that survives the component that
//! started it, a watch on a directory, and the arithmetic that places a floating
//! surface beside the thing it belongs to.
//!
//! They are here so that a region can be a crate of its own. A panel that has these does not need
//! the application around it.

pub mod anchor;
pub mod markdown;

mod clock;
mod erase;
mod task;
mod visible;
mod watch;

pub use crate::clock::{Clock, Job, Pending};
pub use crate::erase::{Erase, leaving_state};
pub use crate::task::detached;
pub use crate::visible::keep_visible;
pub use crate::watch::{Watcher, watch};
