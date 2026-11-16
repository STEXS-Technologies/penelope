//! The same counter model, with captured prediction and optional correction.
#[path = "support/counter.rs"]
mod counter;
use counter::{Counter, DEFINITION};
use penelope::{machine::*, timeline::*};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let machine_scope = MachineScope {
        tenant: MachineTenantId::from_bytes([1; 32]),
        instance: MachineId::from_bytes([2; 32]),
        definition: DEFINITION,
    };
    // Explicit binding of different protocol roles; these are not interchangeable IDs.
    let scope = TimelineScope {
        tenant: TimelineTenantId::from_digest(Digest([11; 32])),
        timeline: TimelineId::from_digest(Digest([12; 32])),
        model: TimelineModelId::from_digest(Digest([13; 32])),
        schema: TimelineSchemaId::from_digest(Digest([14; 32])),
    };
    let model = MachineTimeline::new(Counter, machine_scope, scope)?;
    let machine_genesis = MachineHead {
        scope: machine_scope,
        revision: MachineRevision::ZERO,
    };
    let initial = model.restore(machine_genesis, 0)?;
    let genesis = TimelineHead::new(
        scope,
        TimelineBranchId::ZERO,
        TimelineStep::ZERO,
        TimelineRevision::ZERO,
        None,
    )?;
    let mut live = LiveTimeline::<_, _, 1, 4>::new(
        genesis,
        initial.clone(),
        EffectLimits {
            max_intents: 4,
            max_payload_bytes: 32,
        },
    );
    let valid = |_: &MachineTimelineState<u64>, _: TimelineHead| Ok(());
    let mut retained = Vec::new();
    for (nonce, amount) in [(1_u8, 3_u64), (2, 4)] {
        let input = MachineTimelineInput {
            context: TransitionContext {
                expected: live.read().1.read().0,
                event: MachineEventId::from_bytes([nonce; 32]),
                event_digest: amount.digest().0,
                logical_time: MachineLogicalTime::new(u64::from(nonce)),
            },
            event: amount,
        };
        let record = live.apply_input(
            &model,
            TimelineInputId::from_digest(Digest([nonce; 32])),
            &input,
            &valid,
        )?;
        retained.push(CapturedInput::new(record, input)?);
    }
    println!(
        "predicted={} eligible_effects={}",
        live.read().1.read().1,
        live.eligible_effects().count()
    );

    // Simulated trusted authority replaces the complete genesis state. Real
    // applications authenticate this observation and snapshot independently.
    let server_state = model.restore(machine_genesis, 100)?;
    let snapshot = CapturedSnapshot::capture(
        live.read().0,
        TimelineStep::ZERO,
        TimelineSnapshotId::from_digest(server_state.digest()),
        server_state,
    )?;
    let records: Vec<_> = retained.iter().map(CapturedInput::record).collect();
    let mut correction = live.begin_correction(
        CorrectionRequest {
            correction: Correction::State(snapshot.anchor()),
            inputs: &records,
            observation: AuthorityObservation {
                scope,
                authority: TimelineAuthorityId::from_digest(Digest([99; 32])),
                epoch: AuthorityEpoch::ZERO,
                sequence: AuthoritySequence::new(1),
                step: snapshot.anchor().step,
                digest: snapshot.anchor().digest,
            },
            scratch: initial,
            limits: ReplayLimits {
                max_retained_inputs: 2,
                max_replay_steps: 2,
            },
        },
        &snapshot,
    )?;
    while correction.advance(&model, retained.as_slice(), 1)? == ReplayPhase::Replaying {}
    correction.publish(&valid)?;
    let (head, state) = live.read();
    println!(
        "corrected={} machine_revision={} timeline_revision={} eligible_effects={}",
        state.read().1,
        state.read().0.revision.get(),
        head.revision().get(),
        live.eligible_effects().count()
    );
    // Correction is not effect finality. Nothing is finalized or dispatched here.
    Ok(())
}
