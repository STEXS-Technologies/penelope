#![deny(unsafe_code)]
#![allow(clippy::must_use_candidate)]

#[path = "mod.rs"]
mod entry;

pub use entry::*;

/// Bounded synchronous task contracts for embedded and hosted adapters.
pub use penelope_core::task;
/// Hosted asynchronous task storage boundary.
pub mod tasks;
