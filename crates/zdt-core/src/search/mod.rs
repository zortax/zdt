//! Finding things.
//!
//! Two ways: by name, which is a walk and a fuzzy match, and by content, which is a grep. Both
//! run in this process, and both block. The interface layer above puts them on a worker.
//!
//! Nothing here knows what a picker is. It produces paths, hits and rankings. How they are shown,
//! previewed and opened belongs to the interface.

pub mod files;
pub mod fuzzy;
pub mod grep;

pub use crate::search::files::Walk;
pub use crate::search::grep::{Cancel, Hit, Query};
