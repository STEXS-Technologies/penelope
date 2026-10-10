#![deny(unsafe_code)]
#![allow(clippy::must_use_candidate)]

#[path = "mod.rs"]
mod entry;

pub use entry::*;

/// Optional independently verified task checkpoint-state correlation.
pub mod task;
