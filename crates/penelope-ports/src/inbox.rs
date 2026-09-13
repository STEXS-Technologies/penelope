//! Port trait for event consumption with dedup.
//!
//! Inbox dedup keyed on `(tenant_id, event_id)` (ADR-012 pattern).
