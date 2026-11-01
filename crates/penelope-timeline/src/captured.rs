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
// Ordered contiguous slices use a checked O(1) slot hint per read; other layouts
// retain a linear fallback. Neither lookup path skips descriptor or digest checks.
impl<Input: HistoryPayload> InputStore<Input> for [CapturedInput<Input>] {
    type Error = TimelineError;
    fn read(&self, record: InputRecord) -> Result<BoundInput<'_, Input>, TimelineError> {
        // Contiguous prediction buffers normally map logical steps directly to
        // slots. This is only a hint: arbitrary order, gaps, branches and input
        // revisions remain supported by the exact-descriptor fallback below.
        let indexed = self.first().and_then(|first| {
            record
                .step
                .get()
                .checked_sub(first.record.step.get())
                .and_then(|offset| usize::try_from(offset).ok())
                .and_then(|offset| self.get(offset))
                .filter(|input| input.record == record)
        });
        indexed
            .or_else(|| {
                self.iter()
                    .find(|input| input.record.step == record.step && input.record == record)
            })
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
