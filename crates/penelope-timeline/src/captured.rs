//! Small owned immutable adapters for bounded application buffers.
use crate::*;

/// One owned input bound to its exact descriptor. Construction verifies content,
/// not authority. Payloads must obey the `HistoryPayload` immutability contract.
pub struct CapturedInput<Input> {
    record: InputRecord,
    value: Input,
}
impl<Input: HistoryPayload> CapturedInput<Input> {
    /// Binds an already admitted record to matching captured input bytes.
    /// # Errors
    /// Refuses a mismatched payload digest.
    pub fn new(record: InputRecord, value: Input) -> Result<Self, TimelineError> {
        if value.digest() != record.digest {
            return Err(TimelineError::InputMismatch);
        }
        Ok(Self { record, value })
    }
    /// Immutable descriptor, suitable for a bounded replay-plan buffer.
    pub const fn record(&self) -> InputRecord {
        self.record
    }
}
impl<Input: HistoryPayload> InputStore<Input> for CapturedInput<Input> {
    type Error = TimelineError;
    fn read(&self, record: InputRecord) -> Result<BoundInput<'_, Input>, TimelineError> {
        if record != self.record || self.value.digest() != record.digest {
            return Err(TimelineError::InputMismatch);
        }
        Ok(BoundInput {
            record,
            value: &self.value,
        })
    }
}
impl<Input: HistoryPayload> InputStore<Input> for [CapturedInput<Input>] {
    type Error = TimelineError;
    fn read(&self, record: InputRecord) -> Result<BoundInput<'_, Input>, TimelineError> {
        self.iter()
            .find(|input| input.record == record)
            .ok_or(TimelineError::HistoryUnavailable)?
            .read(record)
    }
}
impl<Input: HistoryPayload, const INPUTS: usize> InputStore<Input>
    for [CapturedInput<Input>; INPUTS]
{
    type Error = TimelineError;
    fn read(&self, record: InputRecord) -> Result<BoundInput<'_, Input>, TimelineError> {
        self.as_slice().read(record)
    }
}

/// One complete isolated snapshot for a small in-memory replay composition.
/// This is not a persistent store or a canonical-live-snapshot authorization.
pub struct CapturedSnapshot<State> {
    anchor: StateAnchor,
    value: State,
}
impl<State: HistoryPayload> CapturedSnapshot<State> {
    /// Captures a trusted post-step state in the current consistency domain.
    /// The caller supplies the stable snapshot identity and authenticates state.
    /// # Errors
    /// Refuses a snapshot beyond the supplied head's logical step.
    pub fn capture(
        head: TimelineHead,
        step: TimelineStep,
        identity: TimelineSnapshotId,
        value: State,
    ) -> Result<Self, TimelineError> {
        if step > head.step() {
            return Err(TimelineError::FutureCorrection);
        }
        let anchor = StateAnchor {
            scope: head.scope(),
            branch: head.branch(),
            step,
            snapshot: identity,
            digest: value.digest(),
        };
        Ok(Self { anchor, value })
    }
    /// Exact immutable restore anchor, including the captured payload digest.
    pub const fn anchor(&self) -> StateAnchor {
        self.anchor
    }
}
impl<State: HistoryPayload + Clone> SnapshotStore<State> for CapturedSnapshot<State> {
    type Error = TimelineError;
    fn restore(
        &self,
        anchor: StateAnchor,
        scratch: &mut State,
    ) -> Result<StateAnchor, TimelineError> {
        if anchor != self.anchor || self.value.digest() != anchor.digest {
            return Err(TimelineError::InputMismatch);
        }
        *scratch = self.value.clone();
        Ok(anchor)
    }
}
