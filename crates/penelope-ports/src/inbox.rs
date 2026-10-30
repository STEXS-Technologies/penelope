//! Read-only inbox recovery. Discovery does not acknowledge or consume inputs.
use crate::PortError;
use async_trait::async_trait;
use penelope_domain::{ProcessId, ProcessInput, TenantId};
use std::num::NonZeroU16;

/// Maximum number of pending inputs returned in one discovery call.
pub const MAX_PENDING_INPUT_BATCH: u16 = 64;

/// Discovers durable inputs not yet consumed by an atomic process commit.
///
/// Input identity is scoped to tenant/process, as in [`crate::Inbox`]. Discovery
/// does not establish a definition binding or causal order. The host validates
/// the payload against its pinned definition before deciding, and authorizes
/// access at its application boundary. Concurrent readers may see the same input;
/// atomic commit CAS and immutable input deduplication arbitrate consumption.
#[async_trait]
pub trait PendingInbox: Send + Sync {
    /// Returns at most `limit` immutable inputs, ordered by input ID. Repeated
    /// reads may return the same batch until committed inputs are consumed.
    /// New inputs may appear between calls; this is not a queue lease or FIFO.
    /// # Errors
    /// Rejects limits above [`MAX_PENDING_INPUT_BATCH`], corrupt records and
    /// unavailable storage. Never treats a read error as an empty inbox.
    async fn pending_inputs(
        &self,
        tenant: &TenantId,
        process: &ProcessId,
        limit: NonZeroU16,
    ) -> Result<Vec<ProcessInput>, PortError>;
}
