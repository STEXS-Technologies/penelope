#![deny(unsafe_code)]
#![allow(clippy::must_use_candidate)]

#[path = "mod.rs"]
mod entry;

pub use entry::*;

/// Recoverable task supervision, independent of the saga engines.
pub mod task;
