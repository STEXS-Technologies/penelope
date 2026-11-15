//! Typed deterministic transitions and bounded in-memory publication.
//!
//! Models return candidate state and stage intents; they never receive mutable
//! published state. Callers supply captured logical context. Purity, independent
//! state ownership and authentic input remain consumer obligations: Rust cannot
//! prevent I/O or interior-mutable aliases in an application model.
//!
//! This module is allocation-free for allocation-free application types. It does
//! not journal events, deduplicate history, authenticate callers or dispatch I/O.

mod contracts;
mod hierarchy;
mod identities;
mod memory;
mod view;

pub use contracts::*;
pub use hierarchy::*;
pub use identities::*;
pub use memory::*;
pub use view::*;
