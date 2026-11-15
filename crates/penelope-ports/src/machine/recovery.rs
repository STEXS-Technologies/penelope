//! Checked recovery bytes are distinct from newly accepted preparation.
use super::*;

/// A structurally verified stored batch. This does not authenticate its source,
/// authorize dispatch, or provide a newly accepted batch for store admission.
/// Obtain bytes from a trusted, authorized store in a coherent read transaction.
pub struct MachineStoredCommit<const INTENTS: usize> {
    batch: MachineCommitBatch<INTENTS>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MachineRecoveryError {
    #[error(transparent)]
    Wire(#[from] MachineWireError),
    #[error("stored machine payload or batch exceeds configured capacity")]
    Capacity,
    #[error("stored machine payload bytes do not match the complete record")]
    PayloadMismatch,
}

impl<const INTENTS: usize> MachineStoredCommit<INTENTS> {
    /// Validates all exact immutable payloads before retaining owned copies.
    /// Reads at most INTENTS + 1 items, even from an unbounded iterator. No model,
    /// codec, event evaluation, source authentication or dispatch occurs here.
    /// # Errors
    /// Refuses malformed wire, excess/missing payloads, hash/charge substitution
    /// and per-payload or aggregate byte limits.
    pub fn decode<'payload>(
        record: &[u8],
        event: &[u8],
        state: &[u8],
        intents: impl IntoIterator<Item = &'payload [u8]>,
        limits: MachinePayloadLimits,
    ) -> Result<Self, MachineRecoveryError> {
        if record.len() > limits.max_record_bytes
            || event.len() > limits.max_event_bytes
            || state.len() > limits.max_state_bytes
        {
            return Err(MachineRecoveryError::Capacity);
        }
        let record = MachineCommitRecord::decode(record)?;
        let header = record.header();
        if Digest::of(event).0 != header.transition.context.event_digest
            || Digest::of(state).0 != header.next_state_digest
        {
            return Err(MachineRecoveryError::PayloadMismatch);
        }
        let mut byte_len = record
            .encoded_len()
            .checked_add(event.len())
            .and_then(|len| len.checked_add(state.len()))
            .filter(|len| *len <= limits.max_batch_bytes)
            .ok_or(MachineRecoveryError::Capacity)?;
        let mut payloads = core::array::from_fn::<_, INTENTS, _>(|_| None);
        let mut source = intents.into_iter();
        for (slot, metadata) in payloads.iter_mut().zip(record.intents()) {
            let bytes = source.next().ok_or(MachineRecoveryError::PayloadMismatch)?;
            if bytes.len() > limits.max_intent_bytes {
                return Err(MachineRecoveryError::Capacity);
            }
            byte_len = byte_len
                .checked_add(bytes.len())
                .filter(|len| *len <= limits.max_batch_bytes)
                .ok_or(MachineRecoveryError::Capacity)?;
            if u64::try_from(bytes.len()).ok() != Some(metadata.payload_bytes)
                || Digest::of(bytes).0 != metadata.digest
            {
                return Err(MachineRecoveryError::PayloadMismatch);
            }
            *slot = Some(bytes);
        }
        if source.next().is_some() {
            return Err(MachineRecoveryError::PayloadMismatch);
        }
        let event = copy_bytes(event).ok_or(MachineRecoveryError::Capacity)?;
        let state = copy_bytes(state).ok_or(MachineRecoveryError::Capacity)?;
        let mut owned_intents = core::array::from_fn(|_| None);
        for (slot, payload) in owned_intents.iter_mut().zip(payloads.into_iter().flatten()) {
            *slot = Some(copy_bytes(payload).ok_or(MachineRecoveryError::Capacity)?);
        }
        Ok(Self {
            batch: MachineCommitBatch {
                record,
                event,
                state,
                intents: owned_intents,
                byte_len,
            },
        })
    }

    /// Read-only recovered bytes; cannot be promoted to a fresh admission Arc.
    pub const fn batch(&self) -> &MachineCommitBatch<INTENTS> {
        &self.batch
    }

    /// Reconstructs exact duplicate coordinates. This DTO is evidence only when
    /// the containing bytes came from the trusted authoritative journal.
    pub const fn receipt(&self) -> MachineCommitReceipt {
        let transition = self.batch.record.header().transition;
        MachineCommitReceipt {
            head: MachineHead {
                scope: transition.context.expected.scope,
                revision: transition.revision,
            },
            event: transition.context.event,
            identity: self.batch.record.identity(),
            disposition: MachineCommitDisposition::AlreadyCommitted,
        }
    }

    /// Restores an isolated native owner from a verified complete state snapshot.
    /// Re-encoding must reproduce every byte; lossy/noncanonical codecs fail.
    /// Does not evaluate an event or restore local pending intents/provenance.
    /// Pending durable effects remain in the authoritative journal/outbox.
    /// # Errors
    /// Refuses configured-scope/model/codec substitution, codec errors, insufficient
    /// scratch, noncanonical state and model invariant or intent-limit violations.
    pub fn restore<M, C>(
        &self,
        model: M,
        codec: &C,
        scope: MachineScope,
        scratch: &mut [u8],
        limits: IntentLimits,
    ) -> Result<MemoryMachine<M, INTENTS>, MachinePreparationError<M::Error, C::Error>>
    where
        M: Machine,
        C: MachineCodec<M>,
    {
        let head = self.receipt().head;
        if head.scope != scope {
            return Err(MachineFailure::Kernel(MachineError::ScopeMismatch).into());
        }
        if model.definition() != scope.definition || codec.definition() != scope.definition {
            return Err(MachineFailure::Kernel(MachineError::DefinitionMismatch).into());
        }
        let state = codec
            .decode_state(self.batch.state())
            .map_err(MachinePreparationError::Codec)?;
        let len = codec
            .encode_state(&state, scratch)
            .map_err(MachinePreparationError::Codec)?;
        if scratch.get(..len) != Some(self.batch.state()) {
            return Err(MachinePreparationError::PayloadMismatch);
        }
        MemoryMachine::restore(model, head, state, limits).map_err(Into::into)
    }
}
