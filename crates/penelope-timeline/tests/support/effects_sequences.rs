//! Slot-independent reference model for interleaved effect operations.
#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing
)]
use penelope_core::task::Digest;
use penelope_timeline::*;
use std::collections::BTreeMap;

struct Command(Vec<u64>);
impl HistoryPayload for Command {
    fn digest(&self) -> Digest {
        let bytes: Vec<_> = self
            .0
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        Digest::of(&bytes)
    }
    fn byte_len(&self) -> u64 {
        u64::try_from(self.0.len()).unwrap() * 8
    }
}
struct Model;
impl RewindableComputation for Model {
    type State = u64;
    type Input = Command;
    type Effect = u64;
    type Error = TimelineError;
    fn step<S: EffectSink<Effect = u64>>(
        &self,
        state: &mut u64,
        command: &Command,
        _context: StepContext,
        sink: &mut S,
    ) -> Result<(), TimelineError> {
        *state = state
            .checked_add(1)
            .ok_or(TimelineError::CounterExhausted)?;
        for value in &command.0 {
            sink.stage(
                TimelineEffectId::from_digest(value.digest()),
                value.digest(),
                8,
                *value,
            )?;
        }
        Ok(())
    }
}
struct Ack {
    reject_after: Option<usize>,
    seen: Vec<EffectDescriptor>,
}
impl EffectHandoff<u64> for Ack {
    type Error = TimelineError;
    fn submit(
        &mut self,
        effect: EligibleEffect<'_, u64>,
    ) -> Result<EffectAdmissionReceipt, TimelineError> {
        if self.reject_after == Some(self.seen.len()) {
            return Err(TimelineError::PublicationUnavailable);
        }
        let descriptor = effect.descriptor();
        if descriptor.digest != effect.payload().digest() {
            return Err(TimelineError::InputMismatch);
        }
        self.seen.push(descriptor);
        Ok(EffectAdmissionReceipt {
            descriptor,
            durable_commit: Digest([8; 32]),
        })
    }
}
const fn scope() -> TimelineScope {
    TimelineScope {
        tenant: TimelineTenantId::from_digest(Digest([1; 32])),
        timeline: TimelineId::from_digest(Digest([2; 32])),
        model: TimelineModelId::from_digest(Digest([3; 32])),
        schema: TimelineSchemaId::from_digest(Digest([4; 32])),
    }
}
/// Same trace runs with zero, one and multiple pending slots. The oracle tracks
/// logical intents by identity, never their array position or iteration order.
pub fn run(trace: &[u8]) {
    run_capacity::<0>(trace);
    run_capacity::<1>(trace);
    run_capacity::<8>(trace);
    run_capacity::<32>(trace);
}
fn run_capacity<const C: usize>(trace: &[u8]) {
    let genesis = TimelineHead::new(
        scope(),
        TimelineBranchId::ZERO,
        TimelineStep::ZERO,
        TimelineRevision::ZERO,
        None,
    )
    .unwrap();
    let mut live = LiveTimeline::<u64, u64, 1, C>::new(
        genesis,
        0,
        EffectLimits {
            max_intents: C,
            max_payload_bytes: u64::try_from(C).unwrap() * 8,
        },
    );
    let mut expected = BTreeMap::<u64, EffectDescriptor>::new();
    let mut state = 0;
    let mut serial = 1_u64;
    let mut sequence = 0_u64;
    let valid = |_state: &u64, _head: TimelineHead| Ok(());
    for byte in trace.iter().take(128) {
        let before = live.read().0;
        match byte % 8 {
            0 | 1 => {
                let value = serial;
                serial += 1;
                let emits = if byte % 8 == 0 { vec![value] } else { vec![] };
                let result = live.apply_input(
                    &Model,
                    TimelineInputId::from_digest(serial.digest()),
                    &Command(emits.clone()),
                    &valid,
                );
                if expected.len() + emits.len() > C {
                    assert!(matches!(
                        result,
                        Err(ApplyError::Timeline(TimelineError::Capacity)
                            | ApplyError::Computation(TimelineError::Capacity))
                    ));
                    assert_eq!(live.read().0, before);
                } else {
                    result.unwrap();
                    state += 1;
                    for emitted in emits {
                        let head = live.read().0;
                        expected.insert(
                            emitted,
                            EffectDescriptor {
                                origin: StepContext {
                                    scope: head.scope(),
                                    branch: head.branch(),
                                    step: head.step(),
                                    revision: head.revision(),
                                },
                                identity: TimelineEffectId::from_digest(emitted.digest()),
                                digest: emitted.digest(),
                                payload_bytes: 8,
                            },
                        );
                    }
                }
            }
            2 => {
                let result = live.finalize_effects(before, before.step());
                if before.effect_finalized() == Some(before.step()) {
                    assert_eq!(result, Err(TimelineError::InvalidFrontier));
                } else {
                    result.unwrap();
                }
            }
            3 | 4 => {
                let reject = byte % 8 == 4;
                let budget = usize::from(byte / 8);
                let fail_after = usize::from(byte / 64);
                let mut eligible: Vec<_> = expected
                    .iter()
                    .filter(|(_, descriptor)| {
                        before
                            .effect_finalized()
                            .is_some_and(|step| descriptor.origin.step <= step)
                    })
                    .map(|(value, descriptor)| (*value, *descriptor))
                    .collect();
                eligible.sort_by_key(|(_, descriptor)| {
                    (descriptor.origin.step, descriptor.identity.digest().0)
                });
                eligible.truncate(budget);
                let mut adapter = Ack {
                    reject_after: if reject { Some(fail_after) } else { None },
                    seen: Vec::new(),
                };
                let result = live.handoff(&mut adapter, budget);
                let confirmed = if reject && fail_after < eligible.len() {
                    assert!(result.is_err());
                    fail_after
                } else {
                    assert_eq!(result.unwrap(), eligible.len());
                    eligible.len()
                };
                assert_eq!(
                    adapter.seen,
                    eligible
                        .iter()
                        .take(confirmed)
                        .map(|(_, descriptor)| *descriptor)
                        .collect::<Vec<_>>()
                );
                for (value, _) in eligible.iter().take(confirmed) {
                    expected.remove(value);
                }
                assert_eq!(live.read().0, before);
            }
            5 => {
                sequence += 1;
                let snapshot = CapturedSnapshot::capture(
                    before,
                    before.step(),
                    TimelineSnapshotId::from_digest(sequence.digest()),
                    state + 10,
                )
                .unwrap();
                let result = live.begin_correction(
                    CorrectionRequest {
                        correction: Correction::State(snapshot.anchor()),
                        inputs: &[],
                        observation: AuthorityObservation {
                            scope: scope(),
                            authority: TimelineAuthorityId::from_digest(Digest([6; 32])),
                            epoch: AuthorityEpoch::ZERO,
                            sequence: AuthoritySequence::new(sequence),
                            step: before.step(),
                            digest: snapshot.anchor().digest,
                        },
                        scratch: 0,
                        limits: ReplayLimits {
                            max_retained_inputs: 0,
                            max_replay_steps: 0,
                        },
                    },
                    &snapshot,
                );
                if before.effect_finalized() == Some(before.step()) {
                    assert!(matches!(
                        result,
                        Err(RestoreError::Timeline(
                            TimelineError::ReconciliationRequired
                        ))
                    ));
                } else {
                    result.unwrap().publish(&valid).unwrap();
                    state += 10;
                    expected.retain(|_, descriptor| descriptor.origin.step < before.step());
                }
            }
            6 => {
                // A duplicate surviving identity or two duplicates within a new
                // step must never publish partially staged state or intents.
                let values = match expected.first_key_value() {
                    Some((value, _)) => vec![*value],
                    None => vec![serial, serial],
                };
                let result = live.apply_input(
                    &Model,
                    TimelineInputId::from_digest(serial.digest()),
                    &Command(values),
                    &valid,
                );
                assert!(result.is_err());
                assert_eq!(live.read().0, before);
            }
            7 => {
                let result = live.apply_input(
                    &Model,
                    TimelineInputId::from_digest(serial.digest()),
                    &Command(vec![]),
                    &|_state: &u64, _head| Err(TimelineError::ValidationFailed),
                );
                assert!(matches!(
                    result,
                    Err(ApplyError::Timeline(TimelineError::ValidationFailed))
                ));
                assert_eq!(live.read().0, before);
            }
            _ => {}
        }
        assert_eq!(*live.read().1, state);
        let actual: BTreeMap<_, _> = live
            .tentative_effects()
            .map(|effect| (effect.payload, effect.descriptor))
            .collect();
        assert_eq!(live.tentative_effects().count(), expected.len());
        assert_eq!(actual, expected);
    }
}
