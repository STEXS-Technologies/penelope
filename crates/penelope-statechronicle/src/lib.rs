//! StateChronicle integration adapter for Penelope.
//!
//! This crate owns only the adapter boundary. It will submit a persisted
//! Penelope action through StateChronicle's verified durable command path and
//! translate a verified, committed canonical event into a Penelope inbox input.
//! It must not perform direct database writes, invent a successful command from
//! an HTTP response, or permit replay to issue a second canonical command.
//!
//! No implementation exists yet. The contract and implementation order are in
//! the repository README and TODO.

#![deny(unsafe_code)]
#![allow(clippy::must_use_candidate)]
