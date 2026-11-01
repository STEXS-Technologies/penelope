//! Sparse/dense pending-intent profiles; durable admission is an in-memory ACK.
use super::{Reporter, head, observation};
use penelope_core::task::Digest;
use penelope_timeline::*;
use std::hint::black_box;

type Live<const C: usize> = LiveTimeline<u64, u64, 1, C>;
struct Model(usize);
impl RewindableComputation for Model {
    type State = u64;
    type Input = u64;
    type Effect = u64;
    type Error = TimelineError;
    fn step<S: EffectSink<Effect = u64>>(
        &self,
        state: &mut u64,
        input: &u64,
        context: StepContext,
        sink: &mut S,
    ) -> Result<(), TimelineError> {
        *state = state
            .checked_add(*input)
            .ok_or(TimelineError::CounterExhausted)?;
        for index in 0..self.0 {
            let value = u64::try_from(index).map_err(|_error| TimelineError::Capacity)?;
            let mut key = [0; 16];
            key[..8].copy_from_slice(&context.step.get().to_le_bytes());
            key[8..].copy_from_slice(&value.to_le_bytes());
            sink.stage(
                TimelineEffectId::from_digest(Digest::of(&key)),
                value.digest(),
                8,
                value,
            )?;
        }
        Ok(())
    }
}
#[allow(clippy::trivially_copy_pass_by_ref, clippy::unnecessary_wraps)] // Validator contract.
fn valid(_state: &u64, _head: TimelineHead) -> Result<(), TimelineError> {
    Ok(())
}
fn empty<const C: usize>() -> Live<C> {
    Live::new(
        head(0),
        0,
        EffectLimits {
            max_intents: C,
            max_payload_bytes: u64::try_from(C).unwrap().checked_mul(8).unwrap(),
        },
    )
}
fn pending<const C: usize>(count: usize, second_step: bool, finalized: bool) -> Live<C> {
    let mut live = empty();
    live.apply_input(
        &Model(count),
        TimelineInputId::from_digest(Digest([21; 32])),
        &1,
        &valid,
    )
    .unwrap();
    if second_step {
        live.apply_input(
            &Model(0),
            TimelineInputId::from_digest(Digest([22; 32])),
            &1,
            &valid,
        )
        .unwrap();
    }
    if finalized {
        live.finalize_effects(live.read().0, live.read().0.step())
            .unwrap();
    }
    live
}
fn correct<const C: usize>(live: &mut Live<C>) -> TimelineHead {
    let current = live.read().0;
    let snapshot = CapturedSnapshot::capture(
        current,
        current.step(),
        TimelineSnapshotId::from_digest(7_u64.digest()),
        7_u64,
    )
    .unwrap();
    // Snapshot capture/hash is included here; no replay is needed at current tick.
    let correction = live
        .begin_correction(
            CorrectionRequest {
                correction: Correction::State(snapshot.anchor()),
                inputs: &[],
                observation: observation(snapshot.anchor()),
                scratch: 0,
                limits: ReplayLimits {
                    max_retained_inputs: 0,
                    max_replay_steps: 0,
                },
            },
            &snapshot,
        )
        .unwrap();
    correction.publish(&valid).unwrap()
}
struct Ack {
    previous: Option<(TimelineStep, [u8; 32])>,
    count: usize,
}
impl EffectHandoff<u64> for Ack {
    type Error = TimelineError;
    fn submit(
        &mut self,
        effect: EligibleEffect<'_, u64>,
    ) -> Result<EffectAdmissionReceipt, TimelineError> {
        let descriptor = effect.descriptor();
        let order = (descriptor.origin.step, descriptor.identity.digest().0);
        if self.previous.is_some_and(|last| last >= order)
            || descriptor.digest != effect.payload().digest()
            || descriptor.payload_bytes != 8
        {
            return Err(TimelineError::InputMismatch);
        }
        self.previous = Some(order);
        self.count += 1;
        Ok(EffectAdmissionReceipt {
            descriptor,
            durable_commit: Digest([23; 32]),
        })
    }
}
fn ack() -> Ack {
    Ack {
        previous: None,
        count: 0,
    }
}

pub(super) fn profiles<const C: usize>(report: &mut impl Reporter) {
    for count in [0, 1, 8, 64, 512].into_iter().filter(|count| *count <= C) {
        // Untimed acceptance checks include both promotion retention boundaries.
        for keep in [false, true] {
            let mut checked = pending::<C>(count, keep, false);
            correct(&mut checked);
            assert_eq!(*checked.read().1, 7);
            assert_eq!(
                checked.tentative_effects().count(),
                if keep { count } else { 0 }
            );
        }
        report.sample(
            &format!("effects_read_cap{C}"),
            1,
            count,
            || pending::<C>(count, false, false),
            |live| {
                live.tentative_effects().fold(0_u64, |sum, effect| {
                    sum.wrapping_add(u64::from(black_box(
                        effect.descriptor.identity.digest().0[0],
                    )))
                })
            },
        );
        report.sample(
            &format!("effects_stage_cap{C}"),
            1,
            count,
            empty::<C>,
            |live| {
                live.apply_input(
                    &Model(count),
                    TimelineInputId::from_digest(Digest([21; 32])),
                    &1,
                    &valid,
                )
                .unwrap()
            },
        );
        report.sample(
            &format!("effects_append_cap{C}"),
            1,
            count,
            || pending::<C>(count, false, false),
            |live| {
                let old = live.read().0;
                let result = live.apply_input(
                    &Model(1),
                    TimelineInputId::from_digest(Digest([22; 32])),
                    &1,
                    &valid,
                );
                if count == C {
                    assert!(matches!(
                        result,
                        Err(ApplyError::Timeline(TimelineError::Capacity))
                    ));
                    assert_eq!(live.read().0, old);
                } else {
                    result.unwrap();
                }
                live.read().0
            },
        );
        for keep in [false, true] {
            report.sample(
                &format!(
                    "effects_promote_{}_cap{C}",
                    if keep { "keep" } else { "drop" }
                ),
                1,
                count,
                || pending::<C>(count, keep, false),
                correct,
            );
        }
        for budget in [1, C] {
            report.sample(
                &format!(
                    "effects_handoff{}_cap{C}",
                    if budget == 1 { "_one" } else { "_all" }
                ),
                1,
                count,
                || pending::<C>(count, false, true),
                |live| {
                    let mut adapter = ack();
                    let admitted = live.handoff(&mut adapter, black_box(budget)).unwrap();
                    assert_eq!(admitted, count.min(budget));
                    assert_eq!(adapter.count, admitted);
                    admitted
                },
            );
        }
    }
}
