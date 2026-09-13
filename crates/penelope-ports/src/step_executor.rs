//! Port trait for step execution.
//!
//! Executes a saga step and records its outcome; idempotent by key
//! `(tenant_id, process_id, step_id, attempt)`.
