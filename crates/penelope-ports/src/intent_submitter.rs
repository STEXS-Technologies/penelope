//! Port trait for submitting commands to a canonical state system.
//!
//! Idempotent on the command id (a command tuple prevents double
//! submission). Penelope never writes canonical state directly — it only
//! submits commands through this port.
