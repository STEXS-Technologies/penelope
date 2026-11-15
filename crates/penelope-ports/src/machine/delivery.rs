//! Durable source identity, leased handoff and typed receiver receipt verification.
use super::*;
use async_trait::async_trait;

/// Exact journal authority for one outgoing intent. Causation hints alone cannot
/// replace any of these coordinates or the immutable payload binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineIntentSource {
    pub head: MachineHead,
    pub event: MachineEventId,
    pub commit: MachineCommitId,
    pub intent: IntentMetadata,
}
impl MachineIntentSource {
    /// Stable full-source identity within a host-pinned receiver namespace. All
    /// coordinates are fixed-width and tagged separately from other protocols.
    /// This binds content; it does not authenticate a source or permit execution.
    pub fn binding_identity(self, consumer: MachineSourceId) -> [u8; 32] {
        let scope = self.head.scope;
        let mut bytes = b"penelope.machine-receiver-source.v1\0".to_vec();
        for value in [
            consumer.bytes(),
            scope.tenant.bytes(),
            scope.instance.bytes(),
            scope.definition.fingerprint.bytes(),
        ] {
            bytes.extend_from_slice(&value);
        }
        bytes.extend_from_slice(&scope.definition.state_schema.to_le_bytes());
        bytes.extend_from_slice(&scope.definition.event_schema.to_le_bytes());
        bytes.extend_from_slice(&self.head.revision.get().to_le_bytes());
        for value in [
            self.event.bytes(),
            self.commit.bytes(),
            self.intent.identity.bytes(),
            self.intent.digest,
        ] {
            bytes.extend_from_slice(&value);
        }
        bytes.extend_from_slice(&self.intent.payload_bytes.to_le_bytes());
        Digest::of(&bytes).0
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineIntentLeaseMetadata {
    pub source: MachineIntentSource,
    pub owner: MachineLeaseOwner,
    pub token: MachineLeaseToken,
    pub expires: MachineLeaseTick,
    pub attempt: MachineDeliveryAttempt,
}
/// Coordination evidence, not permission to execute an arbitrary external effect.
/// Reclaims require reconciliation. Source-authorized idempotent native receivers
/// retain their own admission/execution authority and complete receipt types.
pub struct MachineIntentLease {
    metadata: MachineIntentLeaseMetadata,
    payload: Vec<u8>,
}
impl MachineIntentLease {
    /// Fixed logical metadata charge, not a public wire format or heap estimate.
    pub const METADATA_BYTES: usize = 304;
    /// Structural DTO decoding does not authenticate a lease or its source.
    /// # Errors
    /// Refuses invalid coordinates, payload substitution and configured bounds.
    pub fn decode(
        metadata: MachineIntentLeaseMetadata,
        payload: &[u8],
        max_payload_bytes: usize,
    ) -> Result<Self, MachineStoreError> {
        if payload.len() > max_payload_bytes {
            return Err(MachineStoreError::Capacity);
        }
        if metadata.source.head.revision == MachineRevision::ZERO
            || metadata.token == MachineLeaseToken::ZERO
            || metadata.expires == MachineLeaseTick::ZERO
            || !(1..=65535).contains(&metadata.attempt.get())
            || u64::try_from(payload.len()).ok() != Some(metadata.source.intent.payload_bytes)
            || Digest::of(payload).0 != metadata.source.intent.digest
        {
            return Err(MachineStoreError::Corrupt);
        }
        Ok(Self {
            metadata,
            payload: copy_bytes(payload).ok_or(MachineStoreError::Capacity)?,
        })
    }
    pub const fn metadata(&self) -> MachineIntentLeaseMetadata {
        self.metadata
    }
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
    pub const fn requires_reconciliation(&self) -> bool {
        self.metadata.attempt.get() > 1
    }
    pub const fn byte_len(&self) -> Option<usize> {
        Self::METADATA_BYTES.checked_add(self.payload.len())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineIntentClaim {
    pub scope: MachineScope,
    pub owner: MachineLeaseOwner,
    pub max_intents: u16,
    pub max_bytes: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineOutboxLimits {
    pub lease_ticks: u64,
    pub max_claims: u16,
    pub max_claim_bytes: usize,
    pub max_attempts: u16,
}
/// A trusted bounded host clock, called on the storage worker. Must remain
/// monotonic across reopen. This is not a captured machine-event timestamp.
pub trait MachineLeaseClock: Send + Sync {
    /// # Errors
    /// Fails closed when trusted lease time is unavailable.
    fn now(&self) -> Result<MachineLeaseTick, MachineStoreError>;
}
/// Exact verified receiver admission, not proof that an external effect completed.
/// These public coordinates are not self-authenticating; only a trusted native
/// receipt verifier may establish their authenticity before acknowledgement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineIntentAcceptance {
    pub source: MachineIntentSource,
    pub consumer: MachineSourceId,
    pub receipt_digest: [u8; 32],
}
/// Verifies a receiver's native typed committed receipt against the full source
/// and immutable payload. This method must not execute, restart or submit effects.
/// The verifier owns receiver authentication and authoritative receipt lookup.
#[async_trait]
pub trait MachineIntentAcceptor: Send + Sync {
    type Receipt: Sync;
    /// Stable host-pinned receiver namespace; never inferred from caller receipts.
    fn identity(&self) -> MachineSourceId;
    /// # Errors
    /// Refuses unknown/uncommitted, unauthenticated or substituted native receipts.
    async fn verify(
        &self,
        source: &MachineIntentSource,
        payload: &[u8],
        receipt: &Self::Receipt,
    ) -> Result<MachineIntentAcceptance, MachineStoreError>;
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineIntentStatus {
    pub source: MachineIntentSource,
    pub attempt: MachineDeliveryAttempt,
    /// Receiver pinned on first claim; absent for unclaimed or legacy unknown delivery.
    pub consumer: Option<MachineSourceId>,
    pub accepted: Option<MachineIntentAcceptance>,
}
/// Claims coordinate source-authorized handoff, never direct external execution.
/// Expired/reopened attempts retain uncertainty and require receiver reconciliation.
/// Receiver verification is pinned by the host adapter, never supplied per call.
/// The first claim durably pins the configured consumer before receiver admission.
/// Reclaim/validation/ACK cannot retarget an attempted intent; unknown legacy
/// targets require explicit reconciliation rather than guessing a new receiver.
/// Every method authorizes before returning scoped data. Acknowledgement verifies
/// the native receiver receipt and rechecks exact live fencing under transaction.
#[async_trait]
pub trait MachineIntentOutbox: Send + Sync {
    type Context: Send + Sync;
    type Receipt: Sync;
    /// Trusted receiver namespace pinned by the host adapter.
    fn consumer(&self) -> MachineSourceId;
    async fn claim(
        &self,
        context: &Self::Context,
        request: MachineIntentClaim,
    ) -> Result<Vec<MachineIntentLease>, MachineStoreError>;
    async fn validate(
        &self,
        context: &Self::Context,
        lease: &MachineIntentLease,
    ) -> Result<(), MachineStoreError>;
    async fn status(
        &self,
        context: &Self::Context,
        scope: MachineScope,
        intent: MachineIntentId,
    ) -> Result<Option<MachineIntentStatus>, MachineStoreError>;
    /// Exact acknowledgement retries return the persisted acceptance, even after
    /// expiry. Different receiver/content or a substituted lease conflicts.
    async fn acknowledge(
        &self,
        context: &Self::Context,
        lease: &MachineIntentLease,
        receipt: &Self::Receipt,
    ) -> Result<MachineIntentAcceptance, MachineStoreError>;
}
