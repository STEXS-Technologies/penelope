//! Minimal owned-state input and scoped correction with an immutable input adapter.
use penelope_core::task::Digest;
use penelope_timeline::*;

struct Counter;
impl RewindableComputation for Counter {
    type State = u64;
    type Input = u64;
    type Effect = ();
    type Error = TimelineError;

    fn step<S: EffectSink<Effect = ()>>(
        &self,
        state: &mut u64,
        input: &u64,
        _context: StepContext,
        _effects: &mut S,
    ) -> Result<(), TimelineError> {
        *state = state
            .checked_add(*input)
            .ok_or(TimelineError::ValidationFailed)?;
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let scope = TimelineScope {
        tenant: TimelineTenantId::from_digest(Digest([1; 32])),
        timeline: TimelineId::from_digest(Digest([2; 32])),
        model: TimelineModelId::from_digest(Digest([3; 32])),
        schema: TimelineSchemaId::from_digest(Digest([4; 32])),
    };
    let head = TimelineHead::new(
        scope,
        TimelineBranchId::ZERO,
        TimelineStep::ZERO,
        TimelineRevision::ZERO,
        None,
    )?;
    let mut timeline = LiveTimeline::<u64, (), 1, 0>::new(
        head,
        0,
        EffectLimits {
            max_intents: 0,
            max_payload_bytes: 0,
        },
    );
    let valid = |state: &u64, _head: TimelineHead| {
        if *state <= 100 {
            Ok(())
        } else {
            Err(TimelineError::ValidationFailed)
        }
    };

    // Ordinary input: no snapshot, scratch, context or effect-buffer plumbing.
    let value = 5;
    let record = timeline.apply_input(
        &Counter,
        TimelineInputId::from_digest(Digest([5; 32])),
        &value,
        &valid,
    )?;
    let input = CapturedInput::new(record, value)?;
    if *timeline.read().1 != 5 {
        return Err(TimelineError::ValidationFailed.into());
    }

    // Trust/authentication belongs to the application. Here its admitted evidence
    // says S0 was 10, rather than 0. Replay must consequently produce S1 = 15.
    let snapshot = CapturedSnapshot::capture(
        timeline.read().0,
        TimelineStep::ZERO,
        TimelineSnapshotId::from_digest(Digest([6; 32])),
        10,
    )?;
    let records = [record];
    let mut correction = timeline.begin_correction(
        CorrectionRequest {
            correction: Correction::State(snapshot.anchor()),
            inputs: &records,
            observation: AuthorityObservation {
                scope,
                authority: TimelineAuthorityId::from_digest(Digest([7; 32])),
                epoch: AuthorityEpoch::ZERO,
                sequence: AuthoritySequence::ZERO,
                step: snapshot.anchor().step,
                digest: snapshot.anchor().digest,
            },
            scratch: 0,
            limits: ReplayLimits {
                max_retained_inputs: 1,
                max_replay_steps: 1,
            },
        },
        &snapshot,
    )?;
    // Yield after each bounded call as appropriate for the application's loop.
    while correction.advance(&Counter, &input, 1)? == ReplayPhase::Replaying {}
    correction.publish(&valid)?;
    if *timeline.read().1 != 15 {
        return Err(TimelineError::ValidationFailed.into());
    }
    println!("quickstart: input=5 corrected_genesis=10 live=15");
    Ok(())
}
