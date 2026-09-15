//! Pure Penelope protocol primitives.
//!
//! This crate is intentionally free of I/O, runtime, database, broker, and
//! StateChronicle dependencies. Shared protocol types live in the domain crate;
//! this crate remains a small dependency-stable anchor for future primitives.

#![deny(unsafe_code)]
#![allow(clippy::must_use_candidate)]
