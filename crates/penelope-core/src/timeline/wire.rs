//! Allocation-free independently versioned timeline metadata records.
//!
//! Decoding verifies structure, not authority, payload integrity or journal order.
//! Restore/admission must authenticate records and use the timeline transition API.
use super::*;
use crate::task::Digest;
use thiserror::Error;

/// Exact prefix for every encoded record, independent of saga/task protocols.
pub const TIMELINE_WIRE_TAG: &[u8; 20] = b"penelope.timeline.v1";
/// Largest v1 record (fork with finality, authority and replacement input).
pub const TIMELINE_RECORD_MAX_BYTES: usize = 832;

/// Immutable intent metadata. Eligibility remains a separately authorized frontier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimelineIntentRecord {
    pub scope: TimelineScope,
    pub branch: TimelineBranchId,
    pub step: TimelineStep,
    pub revision: TimelineRevision,
    pub identity: TimelineEffectId,
    pub digest: Digest,
    pub payload_bytes: u64,
}

/// Versioned metadata only; application payloads and state use consumer codecs.
/// A checkpoint is not sufficient for crash recovery without its matching state,
/// retained payloads, pending intents and authenticated admission evidence.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineRecord {
    Checkpoint {
        head: TimelineHead,
        last_branch: TimelineBranchId,
    },
    Input(InputRecord),
    Anchor(StateAnchor),
    Fork {
        expected: TimelineHead,
        branch: TimelineBranchId,
        anchor: StateAnchor,
        replacement: Option<InputRecord>,
    },
    Intent(TimelineIntentRecord),
}

/// Wire refusals do not authorize reconstruction or partial publication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TimelineWireError {
    #[error("unsupported timeline protocol tag/version")]
    UnsupportedVersion,
    #[error("malformed, truncated, noncanonical or trailing timeline bytes")]
    InvalidFormat,
    #[error("insufficient caller-owned encoding capacity")]
    Capacity,
    #[error("invalid timeline record: {0}")]
    InvalidTimeline(#[from] TimelineError),
}

impl TimelineRecord {
    /// Exact canonical length, including tag and variant byte.
    #[must_use]
    pub const fn encoded_len(self) -> usize {
        // Each expression is bounded by the fixed v1 variant maximum (832 bytes).
        #[allow(clippy::arithmetic_side_effects)]
        match self {
            Self::Checkpoint { head, .. } => 21 + head_len(head) + 8,
            Self::Input(_) => 237,
            Self::Anchor(_) => 229,
            Self::Fork {
                expected,
                replacement,
                ..
            } => {
                21 + head_len(expected) + 8 + 208 + 1 + if replacement.is_some() { 216 } else { 0 }
            }
            Self::Intent(_) => 245,
        }
    }
    /// Writes canonical little-endian fixed-width bytes into caller-owned storage.
    /// # Errors
    /// Rejects invalid metadata or capacity before changing the output buffer.
    pub fn encode(self, output: &mut [u8]) -> Result<usize, TimelineWireError> {
        self.validate()?;
        let len = self.encoded_len();
        let output = output.get_mut(..len).ok_or(TimelineWireError::Capacity)?;
        let mut writer = Writer { remaining: output };
        writer.bytes(TIMELINE_WIRE_TAG)?;
        match self {
            Self::Checkpoint { head, last_branch } => {
                writer.bytes(&[1])?;
                writer.head(head)?;
                writer.number(last_branch.get())?;
            }
            Self::Input(record) => {
                writer.bytes(&[2])?;
                writer.input(record)?;
            }
            Self::Anchor(anchor) => {
                writer.bytes(&[3])?;
                writer.anchor(anchor)?;
            }
            Self::Fork {
                expected,
                branch,
                anchor,
                replacement,
            } => {
                writer.bytes(&[4])?;
                writer.head(expected)?;
                writer.number(branch.get())?;
                writer.anchor(anchor)?;
                writer.bytes(&[u8::from(replacement.is_some())])?;
                if let Some(input) = replacement {
                    writer.input(input)?;
                }
            }
            Self::Intent(intent) => {
                writer.bytes(&[5])?;
                writer.scope(intent.scope)?;
                writer.number(intent.branch.get())?;
                writer.number(intent.step.get())?;
                writer.number(intent.revision.get())?;
                writer.bytes(&intent.identity.digest().0)?;
                writer.bytes(&intent.digest.0)?;
                writer.number(intent.payload_bytes)?;
            }
        }
        Ok(len)
    }
    /// Reads precisely one canonical v1 record. No best-effort version conversion.
    /// # Errors
    /// Rejects unknown versions/tags, truncation, invalid flags/frontiers and trailing bytes.
    pub fn decode(input: &[u8]) -> Result<Self, TimelineWireError> {
        let mut reader = Reader { remaining: input };
        if reader.take::<20>()? != *TIMELINE_WIRE_TAG {
            return Err(TimelineWireError::UnsupportedVersion);
        }
        let record = match reader.byte()? {
            1 => Self::Checkpoint {
                head: reader.head()?,
                last_branch: TimelineBranchId::new(reader.number()?),
            },
            2 => Self::Input(reader.input()?),
            3 => Self::Anchor(reader.anchor()?),
            4 => Self::Fork {
                expected: reader.head()?,
                branch: TimelineBranchId::new(reader.number()?),
                anchor: reader.anchor()?,
                replacement: if reader.flag()? {
                    Some(reader.input()?)
                } else {
                    None
                },
            },
            5 => Self::Intent(TimelineIntentRecord {
                scope: reader.scope()?,
                branch: TimelineBranchId::new(reader.number()?),
                step: TimelineStep::new(reader.number()?),
                revision: TimelineRevision::new(reader.number()?),
                identity: TimelineEffectId::from_digest(Digest(reader.take()?)),
                digest: Digest(reader.take()?),
                payload_bytes: reader.number()?,
            }),
            _ => return Err(TimelineWireError::InvalidFormat),
        };
        if !reader.remaining.is_empty() {
            return Err(TimelineWireError::InvalidFormat);
        }
        record.validate()?;
        Ok(record)
    }
    fn validate(self) -> Result<(), TimelineError> {
        match self {
            Self::Checkpoint { head, last_branch } => {
                if last_branch < head.branch() {
                    return Err(TimelineError::Conflict);
                }
            }
            Self::Fork {
                expected,
                branch,
                anchor,
                replacement,
            } => {
                if anchor.scope != expected.scope() || anchor.branch != expected.branch() {
                    return Err(TimelineError::ScopeMismatch);
                }
                if branch <= expected.branch() {
                    return Err(TimelineError::Conflict);
                }
                expected
                    .revision()
                    .checked_next()
                    .ok_or(TimelineError::CounterExhausted)?;
                let corrected = if let Some(record) = replacement {
                    if record.scope != expected.scope() || record.branch != expected.branch() {
                        return Err(TimelineError::ScopeMismatch);
                    }
                    if Some(record.step) != anchor.step.checked_next() {
                        return Err(TimelineError::InputMismatch);
                    }
                    record.step
                } else {
                    anchor.step
                };
                if corrected > expected.step() {
                    return Err(TimelineError::FutureCorrection);
                }
                if expected
                    .effect_finalized()
                    .is_some_and(|frontier| corrected <= frontier)
                {
                    return Err(TimelineError::ReconciliationRequired);
                }
            }
            Self::Input(_) | Self::Anchor(_) | Self::Intent(_) => {}
        }
        Ok(())
    }
}
const fn head_len(head: TimelineHead) -> usize {
    #[allow(clippy::arithmetic_side_effects)]
    {
        154 + if head.effect_finalized().is_some() {
            8
        } else {
            0
        } + if head.authority().is_some() { 216 } else { 0 }
    }
}

struct Writer<'output> {
    remaining: &'output mut [u8],
}
impl Writer<'_> {
    fn bytes(&mut self, value: &[u8]) -> Result<(), TimelineWireError> {
        if value.len() > self.remaining.len() {
            return Err(TimelineWireError::Capacity);
        }
        let (selected, rest) = core::mem::take(&mut self.remaining).split_at_mut(value.len());
        selected.copy_from_slice(value);
        self.remaining = rest;
        Ok(())
    }
    fn number(&mut self, value: u64) -> Result<(), TimelineWireError> {
        self.bytes(&value.to_le_bytes())
    }
    fn scope(&mut self, scope: TimelineScope) -> Result<(), TimelineWireError> {
        self.bytes(&scope.tenant.digest().0)?;
        self.bytes(&scope.timeline.digest().0)?;
        self.bytes(&scope.model.digest().0)?;
        self.bytes(&scope.schema.digest().0)
    }
    fn input(&mut self, record: InputRecord) -> Result<(), TimelineWireError> {
        self.scope(record.scope)?;
        self.number(record.branch.get())?;
        self.number(record.step.get())?;
        self.bytes(&record.identity.digest().0)?;
        self.number(record.revision.get())?;
        self.bytes(&record.digest.0)
    }
    fn anchor(&mut self, anchor: StateAnchor) -> Result<(), TimelineWireError> {
        self.scope(anchor.scope)?;
        self.number(anchor.branch.get())?;
        self.number(anchor.step.get())?;
        self.bytes(&anchor.snapshot.digest().0)?;
        self.bytes(&anchor.digest.0)
    }
    fn observation(&mut self, observation: AuthorityObservation) -> Result<(), TimelineWireError> {
        self.scope(observation.scope)?;
        self.bytes(&observation.authority.digest().0)?;
        self.number(observation.epoch.get())?;
        self.number(observation.sequence.get())?;
        self.number(observation.step.get())?;
        self.bytes(&observation.digest.0)
    }
    fn head(&mut self, head: TimelineHead) -> Result<(), TimelineWireError> {
        self.scope(head.scope())?;
        self.number(head.branch().get())?;
        self.number(head.step().get())?;
        self.number(head.revision().get())?;
        self.bytes(&[u8::from(head.effect_finalized().is_some())])?;
        if let Some(frontier) = head.effect_finalized() {
            self.number(frontier.get())?;
        }
        self.bytes(&[u8::from(head.authority().is_some())])?;
        if let Some(observation) = head.authority() {
            self.observation(observation)?;
        }
        Ok(())
    }
}
struct Reader<'input> {
    remaining: &'input [u8],
}
impl Reader<'_> {
    fn take<const BYTES: usize>(&mut self) -> Result<[u8; BYTES], TimelineWireError> {
        let value = self
            .remaining
            .get(..BYTES)
            .ok_or(TimelineWireError::InvalidFormat)?;
        let mut result = [0; BYTES];
        result.copy_from_slice(value);
        self.remaining = self
            .remaining
            .get(BYTES..)
            .ok_or(TimelineWireError::InvalidFormat)?;
        Ok(result)
    }
    fn byte(&mut self) -> Result<u8, TimelineWireError> {
        let [value] = self.take()?;
        Ok(value)
    }
    fn flag(&mut self) -> Result<bool, TimelineWireError> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(TimelineWireError::InvalidFormat),
        }
    }
    fn number(&mut self) -> Result<u64, TimelineWireError> {
        Ok(u64::from_le_bytes(self.take()?))
    }
    fn scope(&mut self) -> Result<TimelineScope, TimelineWireError> {
        Ok(TimelineScope {
            tenant: TimelineTenantId::from_digest(Digest(self.take()?)),
            timeline: TimelineId::from_digest(Digest(self.take()?)),
            model: TimelineModelId::from_digest(Digest(self.take()?)),
            schema: TimelineSchemaId::from_digest(Digest(self.take()?)),
        })
    }
    fn input(&mut self) -> Result<InputRecord, TimelineWireError> {
        Ok(InputRecord {
            scope: self.scope()?,
            branch: TimelineBranchId::new(self.number()?),
            step: TimelineStep::new(self.number()?),
            identity: TimelineInputId::from_digest(Digest(self.take()?)),
            revision: InputRevision::new(self.number()?),
            digest: Digest(self.take()?),
        })
    }
    fn anchor(&mut self) -> Result<StateAnchor, TimelineWireError> {
        Ok(StateAnchor {
            scope: self.scope()?,
            branch: TimelineBranchId::new(self.number()?),
            step: TimelineStep::new(self.number()?),
            snapshot: TimelineSnapshotId::from_digest(Digest(self.take()?)),
            digest: Digest(self.take()?),
        })
    }
    fn observation(&mut self) -> Result<AuthorityObservation, TimelineWireError> {
        Ok(AuthorityObservation {
            scope: self.scope()?,
            authority: TimelineAuthorityId::from_digest(Digest(self.take()?)),
            epoch: AuthorityEpoch::new(self.number()?),
            sequence: AuthoritySequence::new(self.number()?),
            step: TimelineStep::new(self.number()?),
            digest: Digest(self.take()?),
        })
    }
    fn head(&mut self) -> Result<TimelineHead, TimelineWireError> {
        let scope = self.scope()?;
        let branch = TimelineBranchId::new(self.number()?);
        let step = TimelineStep::new(self.number()?);
        let revision = TimelineRevision::new(self.number()?);
        let finalized = if self.flag()? {
            Some(TimelineStep::new(self.number()?))
        } else {
            None
        };
        let mut head = TimelineHead::new(scope, branch, step, revision, finalized)?;
        if self.flag()? {
            let observation = self.observation()?;
            if observation.scope != scope {
                return Err(TimelineError::ScopeMismatch.into());
            }
            if observation.step > step {
                return Err(TimelineError::FutureCorrection.into());
            }
            head.authority = Some(observation);
        }
        Ok(head)
    }
}
