//! Process instances: append-only outcome log + deterministic projection.
//!
//! `apply(input, def, log) -> Vec<Action>` is the ONLY mutation path; all
//! other process state derives from the outcome log.
