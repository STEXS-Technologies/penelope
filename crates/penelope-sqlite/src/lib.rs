//! Optional SQLite adapter. Supported profile: single host, local filesystem,
//! WAL/FULL durability. Digests detect corruption; they do not authenticate a
//! database against malicious rewriting. Task journals offer synchronous calls or
//! an asynchronous worker view; hosted process/machine ports also isolate I/O
//! on bounded workers. Construct adapters
//! outside latency-sensitive async reactors.
#![deny(unsafe_code)]
mod authority;
mod inbox;
mod lease_counter;
mod leases;
mod lifecycle;
mod machine;
pub use machine::{SqliteMachineOutbox, SqliteMachineStore};
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[path = "../tests/support/write_snapshot.rs"]
mod write_snapshot;

mod async_tasks;
pub use async_tasks::SqliteAsyncTaskJournal;

#[cfg(test)]
#[allow(
    dead_code,
    clippy::unwrap_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]
#[path = "../tests/support/machines.rs"]
mod machine_test_support;
