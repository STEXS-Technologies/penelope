#![no_std]
#![deny(unsafe_code)]
#![allow(clippy::must_use_candidate)]

#[path = "mod.rs"]
mod entry;

#[allow(unused_imports)]
pub use entry::*;

/// Allocation-free, versioned recoverable execution contracts and decisions.
pub mod task;

/// Independent timeline identities and bounded correction planning.
pub mod timeline;
