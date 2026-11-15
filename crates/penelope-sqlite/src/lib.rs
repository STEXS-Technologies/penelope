//! Optional SQLite adapter. Supported profile: single host, local filesystem,
//! WAL/FULL durability. Digests detect corruption; they do not authenticate a
//! database against malicious rewriting. Task journal calls are synchronous;
//! hosted process/machine ports isolate I/O on bounded workers. Construct adapters
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
