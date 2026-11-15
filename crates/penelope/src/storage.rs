//! Backend-neutral durable admission and history contracts.
//!
//! Process commits bind saga inputs, outcomes and actions. Task journals bind
//! execution generations and blobs. They are different atomicity contracts;
//! neither is a general application-machine journal or a durable timeline store.
//! Applications supply adapters and verify committed receipts before dispatch.

/// Bounded discovery of accepted, unconsumed process inputs.
pub use penelope_ports::inbox;
/// Application-machine codecs, prepared commit batches and receipt binding.
pub use penelope_ports::machine;
/// Scoped immutable process payload storage.
pub use penelope_ports::payloads;
/// Saga-specific admission, history, definitions and outbox storage.
pub use penelope_ports::{
    AtomicProcessCommit, AtomicProcessCommitReceipt, AtomicProcessCommitReceiptValidationError,
    DefinitionRegistry, Inbox, InboxAcceptanceReceipt, OutboxStore, ProcessCommitVerifier,
    ProcessStore,
};

/// Task-specific journal contracts for portable and hosted applications.
pub mod task {
    /// Synchronous, allocation-free task journal contract.
    pub use penelope_core::task::runtime::TaskJournal;
    /// Hosted asynchronous task journal contract.
    pub use penelope_ports::tasks::AsyncTaskJournal;
}
