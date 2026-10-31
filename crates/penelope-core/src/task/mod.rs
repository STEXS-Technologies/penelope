//! Allocation-free recoverable execution, independent of saga v1 records.
//! Contracts, decisions and runtime ports are separate domains. The kernel
//! owns isolation and immediate safety; the supervisor owns recovery policy.

mod contracts;
pub mod control;
mod identities;
mod projection;
pub mod runtime;
pub use contracts::*;
pub use identities::*;
pub use projection::*;
