#![deny(unsafe_code)]
#![allow(clippy::must_use_candidate)]

#[path = "mod.rs"]
mod entry;

#[allow(unused_imports)]
pub use entry::*;
