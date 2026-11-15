//! Independent, bounded machine commit metadata. No payload codec or authority.
use super::*;
use sha2::{Digest as _, Sha256};
use thiserror::Error;

pub const MACHINE_WIRE_TAG: &[u8; 19] = b"penelope.machine.v1";
/// Commit record with no causation or intents, including its content identity.
pub const MACHINE_COMMIT_BASE_BYTES: usize = 329;
pub const MACHINE_INTENT_METADATA_BYTES: usize = 72;

/// Application-supplied causal coordinates. These do not authenticate a source
/// or substitute for saga action correlation, task generations or bridge receipts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineCausation {
    pub source: MachineSourceId,
    pub cause: MachineCauseId,
    pub correlation: MachineCorrelationId,
}

/// Metadata bound by one accepted commit. Payload bytes must be independently
/// encoded, retained and checked against these digests at the storage boundary.
/// Constructing this DTO does not prove that a model accepted an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineCommitHeader {
    pub transition: TransitionRecord,
    pub event_type: MachineEventType,
    pub previous_state_digest: [u8; 32],
    pub next_state_digest: [u8; 32],
    pub causation: Option<MachineCausation>,
}

/// Canonical metadata for one complete event/state/ordered-intent batch.
/// INTENTS bounds inline storage and admission work; no allocator is required.
/// Hashes detect content changes, not malicious store rewriting or authorization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineCommitRecord<const INTENTS: usize> {
    header: MachineCommitHeader,
    intents: [Option<IntentMetadata>; INTENTS],
    len: usize,
    payload_bytes: u64,
    encoded_len: usize,
    identity: MachineCommitId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MachineWireError {
    #[error("unsupported machine protocol tag/version")]
    UnsupportedVersion,
    #[error("malformed, truncated, noncanonical or trailing machine bytes")]
    InvalidFormat,
    #[error("machine record capacity, charge or encoding bound exhausted")]
    Capacity,
    #[error("machine revision is not the exact checked successor")]
    InvalidRevision,
    #[error("duplicate intent identity within a machine commit")]
    DuplicateIntent,
    #[error("machine commit content identity does not match")]
    CommitMismatch,
}

impl<const INTENTS: usize> MachineCommitRecord<INTENTS> {
    /// Constructs structurally valid metadata in model intent order. This is not
    /// accepted-transition evidence; durable composition must bind a real ready
    /// candidate, canonical payloads and independently authorized store admission.
    /// # Errors
    /// Refuses non-successor revisions, duplicate intents and count/charge overflow.
    /// Consumes at most INTENTS + 1 metadata items, even from an unbounded iterator.
    pub fn new(
        header: MachineCommitHeader,
        intents: impl IntoIterator<Item = IntentMetadata>,
    ) -> Result<Self, MachineWireError> {
        let mut record = Self::empty(header)?;
        for intent in intents {
            record.push(intent)?;
        }
        record.identity = record.content_identity()?;
        Ok(record)
    }

    pub const fn header(&self) -> MachineCommitHeader {
        self.header
    }
    pub const fn identity(&self) -> MachineCommitId {
        self.identity
    }
    pub const fn len(&self) -> usize {
        self.len
    }
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub const fn payload_bytes(&self) -> u64 {
        self.payload_bytes
    }
    pub const fn encoded_len(&self) -> usize {
        self.encoded_len
    }
    pub fn intents(&self) -> impl Iterator<Item = &IntentMetadata> {
        self.intents.iter().take(self.len).flatten()
    }

    /// Encodes exactly one v1 commit into a caller-owned prefix. Bytes beyond
    /// that prefix are untouched; insufficient capacity changes no output bytes.
    /// # Errors
    /// Refuses insufficient output capacity before writing the first byte.
    pub fn encode(&self, output: &mut [u8]) -> Result<usize, MachineWireError> {
        let mut remaining = output
            .get_mut(..self.encoded_len)
            .ok_or(MachineWireError::Capacity)?;
        self.emit(&mut |bytes| {
            crate::__private::put_bytes(&mut remaining, bytes).ok_or(MachineWireError::Capacity)
        })?;
        crate::__private::put_bytes(&mut remaining, &self.identity.bytes())
            .ok_or(MachineWireError::Capacity)?;
        Ok(self.encoded_len)
    }

    /// Decodes one canonical record with bounded intent storage. No payload is
    /// decoded, no event replayed and no authentication or commit is inferred.
    /// # Errors
    /// Refuses wrong versions/variants, truncation, flags, trailing bytes, invalid
    /// revisions, duplicate/oversized batches and a mismatched content identity.
    pub fn decode(input: &[u8]) -> Result<Self, MachineWireError> {
        let mut reader = Reader { remaining: input };
        if reader.take::<19>()? != *MACHINE_WIRE_TAG {
            return Err(MachineWireError::UnsupportedVersion);
        }
        if reader.take::<1>()? != [1] {
            return Err(MachineWireError::InvalidFormat);
        }
        let header = MachineCommitHeader {
            transition: TransitionRecord {
                context: TransitionContext {
                    expected: reader.head()?,
                    event: MachineEventId::from_bytes(reader.take()?),
                    event_digest: reader.take()?,
                    logical_time: MachineLogicalTime::new(reader.number()?),
                },
                revision: MachineRevision::new(reader.number()?),
                transition: TransitionId::new(reader.number()?),
            },
            event_type: MachineEventType::new(reader.number()?),
            previous_state_digest: reader.take()?,
            next_state_digest: reader.take()?,
            causation: match reader.take::<1>()? {
                [0] => None,
                [1] => Some(MachineCausation {
                    source: MachineSourceId::from_bytes(reader.take()?),
                    cause: MachineCauseId::from_bytes(reader.take()?),
                    correlation: MachineCorrelationId::from_bytes(reader.take()?),
                }),
                _ => return Err(MachineWireError::InvalidFormat),
            },
        };
        let count = usize::try_from(u32::from_le_bytes(reader.take()?))
            .map_err(|_overflow| MachineWireError::Capacity)?;
        if count > INTENTS {
            return Err(MachineWireError::Capacity);
        }
        // Reject impossible/trailing bodies before any loop over a supplied count.
        let tail = count
            .checked_mul(MACHINE_INTENT_METADATA_BYTES)
            .and_then(|bytes| bytes.checked_add(32))
            .ok_or(MachineWireError::Capacity)?;
        if reader.remaining.len() != tail {
            return Err(MachineWireError::InvalidFormat);
        }
        let mut record = Self::empty(header)?;
        for _ in 0..count {
            record.push(IntentMetadata {
                identity: MachineIntentId::from_bytes(reader.take()?),
                digest: reader.take()?,
                payload_bytes: reader.number()?,
            })?;
        }
        let identity = MachineCommitId::from_bytes(reader.take()?);
        record.identity = record.content_identity()?;
        if identity != record.identity {
            return Err(MachineWireError::CommitMismatch);
        }
        Ok(record)
    }

    fn empty(header: MachineCommitHeader) -> Result<Self, MachineWireError> {
        if header.transition.context.expected.revision.checked_next()
            != Some(header.transition.revision)
        {
            return Err(MachineWireError::InvalidRevision);
        }
        let encoded_len = MACHINE_COMMIT_BASE_BYTES
            .checked_add(if header.causation.is_some() { 96 } else { 0 })
            .ok_or(MachineWireError::Capacity)?;
        Ok(Self {
            header,
            intents: core::array::from_fn(|_| None),
            len: 0,
            payload_bytes: 0,
            encoded_len,
            identity: MachineCommitId::from_bytes([0; 32]),
        })
    }
    fn push(&mut self, intent: IntentMetadata) -> Result<(), MachineWireError> {
        let (len, payload_bytes) = crate::__private::admit_intent::<INTENTS, _, _>(
            self.len,
            self.payload_bytes,
            (INTENTS, u64::MAX),
            self.intents().map(|old| old.identity),
            intent.identity,
            intent.payload_bytes,
            (
                MachineWireError::Capacity,
                MachineWireError::DuplicateIntent,
            ),
        )?;
        u32::try_from(len).map_err(|_overflow| MachineWireError::Capacity)?;
        let encoded_len = self
            .encoded_len
            .checked_add(MACHINE_INTENT_METADATA_BYTES)
            .ok_or(MachineWireError::Capacity)?;
        let slot = self
            .intents
            .get_mut(self.len)
            .ok_or(MachineWireError::Capacity)?;
        *slot = Some(intent);
        self.len = len;
        self.payload_bytes = payload_bytes;
        self.encoded_len = encoded_len;
        Ok(())
    }
    fn content_identity(&self) -> Result<MachineCommitId, MachineWireError> {
        let mut hash = Sha256::new();
        self.emit(&mut |bytes| {
            hash.update(bytes);
            Ok(())
        })?;
        Ok(MachineCommitId::from_bytes(hash.finalize().into()))
    }
    fn emit(
        &self,
        bytes: &mut impl FnMut(&[u8]) -> Result<(), MachineWireError>,
    ) -> Result<(), MachineWireError> {
        bytes(MACHINE_WIRE_TAG)?;
        bytes(&[1])?;
        let record = self.header.transition;
        let head = record.context.expected;
        bytes(&head.scope.tenant.bytes())?;
        bytes(&head.scope.instance.bytes())?;
        bytes(&head.scope.definition.fingerprint.bytes())?;
        bytes(&head.scope.definition.state_schema.to_le_bytes())?;
        bytes(&head.scope.definition.event_schema.to_le_bytes())?;
        bytes(&head.revision.get().to_le_bytes())?;
        bytes(&record.context.event.bytes())?;
        bytes(&record.context.event_digest)?;
        bytes(&record.context.logical_time.get().to_le_bytes())?;
        bytes(&record.revision.get().to_le_bytes())?;
        bytes(&record.transition.get().to_le_bytes())?;
        bytes(&self.header.event_type.get().to_le_bytes())?;
        bytes(&self.header.previous_state_digest)?;
        bytes(&self.header.next_state_digest)?;
        bytes(&[u8::from(self.header.causation.is_some())])?;
        if let Some(cause) = self.header.causation {
            bytes(&cause.source.bytes())?;
            bytes(&cause.cause.bytes())?;
            bytes(&cause.correlation.bytes())?;
        }
        let count = u32::try_from(self.len).map_err(|_overflow| MachineWireError::Capacity)?;
        bytes(&count.to_le_bytes())?;
        for intent in self.intents() {
            bytes(&intent.identity.bytes())?;
            bytes(&intent.digest)?;
            bytes(&intent.payload_bytes.to_le_bytes())?;
        }
        Ok(())
    }
}

struct Reader<'input> {
    remaining: &'input [u8],
}
impl Reader<'_> {
    fn take<const BYTES: usize>(&mut self) -> Result<[u8; BYTES], MachineWireError> {
        crate::__private::take_bytes(&mut self.remaining).ok_or(MachineWireError::InvalidFormat)
    }
    fn number(&mut self) -> Result<u64, MachineWireError> {
        Ok(u64::from_le_bytes(self.take()?))
    }
    fn head(&mut self) -> Result<MachineHead, MachineWireError> {
        Ok(MachineHead {
            scope: MachineScope {
                tenant: MachineTenantId::from_bytes(self.take()?),
                instance: MachineId::from_bytes(self.take()?),
                definition: MachineDefinition {
                    fingerprint: MachineDefinitionId::from_bytes(self.take()?),
                    state_schema: u32::from_le_bytes(self.take()?),
                    event_schema: u32::from_le_bytes(self.take()?),
                },
            },
            revision: MachineRevision::new(self.number()?),
        })
    }
}
