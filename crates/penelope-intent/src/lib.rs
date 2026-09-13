//! Inbound transport-to-domain boundary for Penelope.
//!
//! Future code validates versioned input DTOs before the application layer sees
//! them. HTTP, RPC, broker, database, and authentication implementations do
//! not belong in this crate.

#![deny(unsafe_code)]
#![allow(clippy::must_use_candidate)]
