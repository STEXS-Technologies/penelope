//! Recoverable execution with explicit recovery permissions and generation fencing.
//!
//! The portable control and hosted supervisor share the existing task policy:
//! journal before launch, fence interrupted generations, and require independent
//! reconciliation for ambiguous effects. Adding facade paths does not authorize
//! a retry, skip recovery gates or change task history encoding.

/// Allocation-free contracts, decisions, control and runtime ports.
pub use penelope_core::task::*;
/// Hosted bounded supervisor using injected journal and runtime adapters.
pub use penelope_executor::task::TaskSupervisor;
