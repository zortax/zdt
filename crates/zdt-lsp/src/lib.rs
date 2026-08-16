//! The language-server client pool.
//!
//! One client per server and project root, each driven by an `async-lsp` main loop on the
//! background executor. Requests are issued from the interface thread and answered there.

pub mod client;
pub mod convert;
pub mod diagnostics;
pub mod pool;
pub mod registry;

pub use crate::client::{Client, ClientError, Notice, Symbol};
pub use crate::convert::Encoding;
pub use crate::diagnostics::{Counts, Store};
pub use crate::pool::{Asked, Pool};
pub use crate::registry::{Wanted, wanted_for};
