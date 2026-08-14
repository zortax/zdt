//! The language-server client pool.
//!
//! One client per server and project root, each driven by an `async-lsp` main loop on the
//! background executor. Requests are issued from the interface thread and answered there.
