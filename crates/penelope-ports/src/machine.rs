//! Checked preparation for machine journals. No store or dispatcher is included.
use penelope_core::{machine::*, task::Digest};
use std::sync::Arc;
use thiserror::Error;

mod recovery;
pub use recovery::*;

/// Application-owned canonical payload interpretation, pinned to the complete
/// model definition. Encoding must be deterministic and include all relevant
/// state/event/effect data. Implementations must not consult mutable host state,
/// retain mutable aliases or perform I/O. Output is restricted to supplied slices.
/// Decoders must refuse unknown tags, wrong schemas, truncation and trailing bytes.
/// The trait imposes no Clone, Serde or equality bound on application types.
pub trait MachineCodec<M: Machine> {
    type Error;
    fn definition(&self) -> MachineDefinition;
    fn event_type(&self, event: &M::Event) -> MachineEventType;
    /// # Errors
    /// Refuses invalid state or insufficient output capacity.
    fn encode_state(&self, state: &M::State, output: &mut [u8]) -> Result<usize, Self::Error>;
    /// # Errors
    /// Refuses an unknown/incomplete state encoding or incompatible schema.
    fn decode_state(&self, input: &[u8]) -> Result<M::State, Self::Error>;
    /// # Errors
    /// Refuses invalid events or insufficient output capacity.
    fn encode_event(&self, event: &M::Event, output: &mut [u8]) -> Result<usize, Self::Error>;
    /// # Errors
    /// Refuses an unknown event type or noncanonical/incompatible bytes.
    fn decode_event(&self, kind: MachineEventType, input: &[u8]) -> Result<M::Event, Self::Error>;
    /// # Errors
    /// Refuses invalid effects or insufficient output capacity.
    fn encode_effect(&self, effect: &M::Effect, output: &mut [u8]) -> Result<usize, Self::Error>;
    /// # Errors
    /// Refuses an unknown/incomplete effect encoding or incompatible schema.
    fn decode_effect(&self, input: &[u8]) -> Result<M::Effect, Self::Error>;
}

/// Admission limits for exact encoded bytes, not application heap accounting.
/// The batch bound includes wire metadata, event, next state and all intent bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachinePayloadLimits {
    pub max_record_bytes: usize,
    pub max_event_bytes: usize,
    pub max_state_bytes: usize,
    pub max_intent_bytes: usize,
    pub max_batch_bytes: usize,
}

/// Caller-owned bounded encoding scratch. State scratch is reused after hashing
/// the predecessor for CAS; the store must already retain that predecessor.
pub struct MachineCommitBuffers<'buffers> {
    pub state: &'buffers mut [u8],
    pub event: &'buffers mut [u8],
    pub intent: &'buffers mut [u8],
}

pub struct MachineCommitInput<'input, Event, Codec> {
    pub event: &'input Event,
    pub context: TransitionContext,
    pub codec: &'input Codec,
    pub causation: Option<MachineCausation>,
}

/// Immutable exact request suitable for retention across async worker/ACK loss.
/// Obtained from accepted preparation, not arbitrary public wire DTOs. Cloning
/// its Arc shares bytes; it neither clones a ready handle nor authorizes dispatch.
pub struct MachineCommitBatch<const INTENTS: usize> {
    record: MachineCommitRecord<INTENTS>,
    event: Vec<u8>,
    state: Vec<u8>,
    intents: [Option<Vec<u8>>; INTENTS],
    byte_len: usize,
}
impl<const INTENTS: usize> MachineCommitBatch<INTENTS> {
    pub const fn record(&self) -> &MachineCommitRecord<INTENTS> {
        &self.record
    }
    pub fn event(&self) -> &[u8] {
        &self.event
    }
    pub fn state(&self) -> &[u8] {
        &self.state
    }
    pub const fn byte_len(&self) -> usize {
        self.byte_len
    }
    pub fn intents(&self) -> impl Iterator<Item = (&IntentMetadata, &[u8])> {
        self.record
            .intents()
            .zip(self.intents.iter().flatten().map(Vec::as_slice))
    }
}

#[derive(Debug, Error)]
pub enum MachinePreparationError<ModelError, CodecError> {
    #[error(transparent)]
    Machine(#[from] MachineFailure<ModelError>),
    #[error("application machine payload encoding failed")]
    Codec(CodecError),
    #[error(transparent)]
    Wire(#[from] MachineWireError),
    #[error("machine payload, record or batch capacity exhausted")]
    Capacity,
    #[error("encoded machine payload does not match its supplied digest/charge")]
    PayloadMismatch,
}

/// Commit status supplied by a trusted store. Both variants require the same
/// complete batch binding; neither establishes external effect execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineCommitDisposition {
    Committed,
    AlreadyCommitted,
}

/// Store DTO, not self-authenticating evidence. Obtain from an authorized trusted
/// store or authoritative lookup; a matching caller-forged DTO proves no durability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineCommitReceipt {
    pub head: MachineHead,
    pub event: MachineEventId,
    pub identity: MachineCommitId,
    pub disposition: MachineCommitDisposition,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("machine commit receipt does not match the exact prepared batch")]
pub struct MachineReceiptError;

/// Owns the original exclusive ready handle and retained immutable commit bytes.
/// Drop aborts local publication; it cannot undo an already committed store write.
/// A failed/lost commit ACK requires authoritative lookup of the exact event/batch.
///
/// ```compile_fail,E0382
/// use penelope_core::machine::Machine;
/// use penelope_ports::machine::{PreparedMachineCommit, MachineCommitReceipt};
/// fn twice<M: Machine, const N: usize>(ready: PreparedMachineCommit<'_, M, N>, receipt: MachineCommitReceipt) {
///     let _ = ready.publish(receipt);
///     let _ = ready.publish(receipt);
/// }
/// ```
#[must_use = "commit and verify the exact receipt, or drop to abort local publication"]
pub struct PreparedMachineCommit<'owner, M: Machine, const INTENTS: usize> {
    ready: PreparedTransition<'owner, M, INTENTS>,
    batch: Arc<MachineCommitBatch<INTENTS>>,
}
impl<M: Machine, const INTENTS: usize> PreparedMachineCommit<'_, M, INTENTS> {
    pub fn batch(&self) -> Arc<MachineCommitBatch<INTENTS>> {
        Arc::clone(&self.batch)
    }
    /// Verifies exact receipt coordinates, then consumes the original local
    /// publication handle. The application must authenticate the receipt source.
    /// A matching receipt proves that batch was committed, not that it is still
    /// the newest store head; later writers remain fenced by the store's CAS.
    /// # Errors
    /// Refuses a substituted scope/head, event or complete batch identity.
    pub fn publish(
        self,
        receipt: MachineCommitReceipt,
    ) -> Result<TransitionRecord, MachineReceiptError> {
        let record = self.batch.record.header().transition;
        if receipt.head
            != (MachineHead {
                scope: record.context.expected.scope,
                revision: record.revision,
            })
            || receipt.event != record.context.event
            || receipt.identity != self.batch.record.identity()
        {
            return Err(MachineReceiptError);
        }
        Ok(self.ready.publish())
    }
}

impl<M: Machine, const INTENTS: usize> PreparedDecision for PreparedMachineCommit<'_, M, INTENTS> {
    type State = M::State;
    type Provenance = TransitionRecord;
    type Intent = ProposedIntent<M::Effect>;
    fn before(&self) -> Option<&Self::State> {
        self.ready.before()
    }
    fn candidate(&self) -> &Self::State {
        self.ready.state()
    }
    fn provenance(&self) -> &Self::Provenance {
        self.ready.provenance()
    }
    fn intents(&self) -> impl Iterator<Item = &Self::Intent> {
        self.ready.intents()
    }
}

#[allow(clippy::large_enum_variant)] // Preserve caller-sized inline application state/intents.
pub enum MachineCommitDecision<'owner, M: Machine, const INTENTS: usize> {
    Prepared(PreparedMachineCommit<'owner, M, INTENTS>),
    Rejected { reason: RejectCode },
    Unhandled,
}

/// Encodes and binds the very event evaluated by the native memory owner. No
/// independent model interpreter, state clone or store mutation is introduced.
/// The application must obey model/codec purity and isolated-state contracts.
/// # Errors
/// Refuses stale/incompatible scope/definition, encoding/digest/charge failures,
/// model/invariant failure and byte/count bounds. Live state remains unchanged.
pub fn prepare_machine_commit<'owner, M, C, const INTENTS: usize>(
    owner: &'owner mut MemoryMachine<M, INTENTS>,
    input: &MachineCommitInput<'_, M::Event, C>,
    buffers: &mut MachineCommitBuffers<'_>,
    limits: MachinePayloadLimits,
) -> Result<MachineCommitDecision<'owner, M, INTENTS>, MachinePreparationError<M::Error, C::Error>>
where
    M: Machine,
    C: MachineCodec<M>,
{
    let (head, previous) = owner.read();
    if input.context.expected.scope != head.scope {
        return Err(MachineFailure::Kernel(MachineError::ScopeMismatch).into());
    }
    if input.context.expected.revision != head.revision {
        return Err(MachineFailure::Kernel(MachineError::StaleHead).into());
    }
    if input.codec.definition() != head.scope.definition {
        return Err(MachineFailure::Kernel(MachineError::DefinitionMismatch).into());
    }
    let state_output = buffers
        .state
        .get_mut(..limits.max_state_bytes)
        .ok_or(MachinePreparationError::Capacity)?;
    let event_output = buffers
        .event
        .get_mut(..limits.max_event_bytes)
        .ok_or(MachinePreparationError::Capacity)?;
    let intent_output = buffers
        .intent
        .get_mut(..limits.max_intent_bytes)
        .ok_or(MachinePreparationError::Capacity)?;
    let previous_len = input
        .codec
        .encode_state(previous, state_output)
        .map_err(MachinePreparationError::Codec)?;
    let previous_bytes = state_output
        .get(..previous_len)
        .ok_or(MachinePreparationError::Capacity)?;
    let previous_state_digest = Digest::of(previous_bytes).0;
    let event_len = input
        .codec
        .encode_event(input.event, event_output)
        .map_err(MachinePreparationError::Codec)?;
    let event_bytes = event_output
        .get(..event_len)
        .ok_or(MachinePreparationError::Capacity)?;
    if Digest::of(event_bytes).0 != input.context.event_digest {
        return Err(MachinePreparationError::PayloadMismatch);
    }
    let event_type = input.codec.event_type(input.event);
    let ready = match owner.prepare(input.event, input.context)? {
        PrepareDecision::Ready(ready) => ready,
        PrepareDecision::Rejected { reason } => {
            return Ok(MachineCommitDecision::Rejected { reason });
        }
        PrepareDecision::Unhandled => return Ok(MachineCommitDecision::Unhandled),
    };
    let state_len = input
        .codec
        .encode_state(ready.state(), state_output)
        .map_err(MachinePreparationError::Codec)?;
    let state_bytes = state_output
        .get(..state_len)
        .ok_or(MachinePreparationError::Capacity)?;
    let record = MachineCommitRecord::new(
        MachineCommitHeader {
            transition: ready.record(),
            event_type,
            previous_state_digest,
            next_state_digest: Digest::of(state_bytes).0,
            causation: input.causation,
        },
        ready.intents().map(|intent| intent.metadata),
    )?;
    if record.encoded_len() > limits.max_record_bytes {
        return Err(MachinePreparationError::Capacity);
    }
    // Preflight the complete declared batch before retaining owned payloads.
    // Per-intent encoding below must match both this charge and the digest.
    for metadata in record.intents() {
        if usize::try_from(metadata.payload_bytes)
            .ok()
            .filter(|len| *len <= limits.max_intent_bytes)
            .is_none()
        {
            return Err(MachinePreparationError::Capacity);
        }
    }
    let intent_bytes = usize::try_from(record.payload_bytes())
        .map_err(|_overflow| MachinePreparationError::Capacity)?;
    let byte_len = record
        .encoded_len()
        .checked_add(event_bytes.len())
        .and_then(|bytes| bytes.checked_add(state_bytes.len()))
        .and_then(|bytes| bytes.checked_add(intent_bytes))
        .filter(|bytes| *bytes <= limits.max_batch_bytes)
        .ok_or(MachinePreparationError::Capacity)?;
    let event = copy_bytes(event_bytes).ok_or(MachinePreparationError::Capacity)?;
    let state = copy_bytes(state_bytes).ok_or(MachinePreparationError::Capacity)?;
    let mut intents = core::array::from_fn(|_| None);
    for (slot, intent) in intents.iter_mut().zip(ready.intents()) {
        let len = input
            .codec
            .encode_effect(&intent.payload, intent_output)
            .map_err(MachinePreparationError::Codec)?;
        let bytes = intent_output
            .get(..len)
            .ok_or(MachinePreparationError::Capacity)?;
        if u64::try_from(bytes.len()).ok() != Some(intent.metadata.payload_bytes)
            || Digest::of(bytes).0 != intent.metadata.digest
        {
            return Err(MachinePreparationError::PayloadMismatch);
        }
        *slot = Some(copy_bytes(bytes).ok_or(MachinePreparationError::Capacity)?);
    }
    Ok(MachineCommitDecision::Prepared(PreparedMachineCommit {
        ready,
        batch: Arc::new(MachineCommitBatch {
            record,
            event,
            state,
            intents,
            byte_len,
        }),
    }))
}

fn copy_bytes(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut owned = Vec::new();
    owned.try_reserve_exact(bytes.len()).ok()?;
    owned.extend_from_slice(bytes);
    Some(owned)
}
