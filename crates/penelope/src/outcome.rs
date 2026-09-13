//! Step outcomes and the outcome log (schema `penelope.process.outcome.v0`).
//!
//! Side-effect outcomes are recorded, never recomputed; a step moves
//! `pending → running → terminal` exactly once.
