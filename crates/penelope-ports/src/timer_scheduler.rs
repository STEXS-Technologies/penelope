//! Port trait for durable timer scheduling.
//!
//! Waits, deadlines, and retry backoff; a firing emits a `TimerFired` event
//! that enters the process stream and is replayed as data.
