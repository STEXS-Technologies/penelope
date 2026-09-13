//! Versioned, immutable saga definitions (schema `penelope.process.def.v0`).
//!
//! A saga definition pins `def_version` at instance start so code evolution
//! never retroactively changes running processes.
