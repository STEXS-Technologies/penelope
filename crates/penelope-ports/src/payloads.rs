//! Immutable event/evidence bytes referenced by process outcome digests.
use crate::PortError;
use async_trait::async_trait;
use penelope_domain::{ContentDigest, ProcessScope};

/// Durable scoped payload storage for replay. Write bytes before committing an
/// outcome that references them. A failed/lost write acknowledgement is ambiguous;
/// repeat the same immutable write. Unreferenced bytes may remain after a crash.
/// Reads must enforce configured byte limits before allocation, verify content
/// identity and reject missing/corrupt bytes. Digests do not authenticate writers.
#[async_trait]
pub trait ProcessPayloadStore: Send + Sync {
    /// Durably stores exact content, idempotently within the complete scope.
    /// Returns the accepted digest; callers must validate it before log commit.
    async fn put_payload(
        &self,
        scope: &ProcessScope,
        digest: ContentDigest,
        bytes: &[u8],
    ) -> Result<ContentDigest, PortError>;

    /// Reads immutable bytes for replay. Missing committed evidence is an error,
    /// never an empty event or permission to redispatch an external effect.
    async fn read_payload(
        &self,
        scope: &ProcessScope,
        digest: ContentDigest,
    ) -> Result<Vec<u8>, PortError>;
}
