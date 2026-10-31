//! Optional SQLite adapter. Supported profile: single host, local filesystem,
//! WAL/FULL durability. Digests detect corruption; they do not authenticate a
//! database against malicious rewriting. Calls are synchronous: isolate them
//! from async reactor threads in the consuming application.
#![deny(unsafe_code)]
mod authority;
mod inbox;
mod leases;
mod lifecycle;
mod payloads;
mod process;
mod schema;
mod task_decode;
mod tasks;
mod worker;
pub use lifecycle::{TaskRetirement, WalCheckpoint};
pub use process::{SqliteLeaseClock, SqliteProcessStore};
pub use schema::SqliteLimits;
pub use tasks::SqliteTaskJournal;
