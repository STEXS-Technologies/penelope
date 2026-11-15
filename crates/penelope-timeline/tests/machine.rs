//! Native-machine differential checks and real timeline correction/finality.
#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::panic_in_result_fn,
    clippy::unwrap_in_result,
    clippy::shadow_unrelated
)]
use penelope_core::{machine::*, task::Digest};
use penelope_timeline::*;

const DEFINITION: MachineDefinition = MachineDefinition {
    fingerprint: MachineDefinitionId::from_bytes([3; 32]),
    state_schema: 1,
    event_schema: 2,
};
const MACHINE: MachineScope = MachineScope {
    tenant: MachineTenantId::from_bytes([1; 32]),
    instance: MachineId::from_bytes([2; 32]),
    definition: DEFINITION,
};
const SCOPE: TimelineScope = TimelineScope {
    tenant: TimelineTenantId::from_digest(Digest([11; 32])),
    timeline: TimelineId::from_digest(Digest([12; 32])),
    model: TimelineModelId::from_digest(Digest([13; 32])),
    schema: TimelineSchemaId::from_digest(Digest([14; 32])),
};
const LIMITS: EffectLimits = EffectLimits {
    max_intents: 128,
    max_payload_bytes: 1024,
};
type Live = LiveTimeline<MachineTimelineState<u64>, MachineTimelineIntent<u64>, 1, 128>;

struct Counter;
impl Machine for Counter {
    type State = u64;
    type Event = u64;
    type Effect = u64;
    type Error = &'static str;
    fn definition(&self) -> MachineDefinition {
        DEFINITION
    }
    fn validate(&self, state: &u64) -> Result<(), Self::Error> {
        if *state == u64::MAX {
            Err("invalid state")
        } else {
            Ok(())
        }
    }
    fn transition<S: IntentSink<u64>>(
        &self,
        current: &u64,
        event: &u64,
        context: &TransitionContext,
        sink: &mut S,
    ) -> Result<Transition<u64>, Self::Error> {
        // Even refusals/errors try staging first, to test complete aborts.
        let _ignored = sink.stage(ProposedIntent {
            metadata: IntentMetadata {
                identity: MachineIntentId::from_bytes(context.event.bytes()),
                digest: context.event_digest,
                payload_bytes: 8,
            },
            payload: *event,
        });
        match *event {
            0 => Ok(Transition::Rejected {
                reason: RejectCode::new(41),
            }),
            1 => Ok(Transition::Unhandled),
            2 => Err("model error"),
            3 => Ok(Transition::Accepted {
                next: u64::MAX,
                transition: TransitionId::new(7),
            }),
            _ => Ok(Transition::Accepted {
                next: current + event + context.logical_time.get(),
                transition: TransitionId::new(7),
            }),
        }
    }
}
fn adapter() -> MachineTimeline<Counter> {
    MachineTimeline::new(Counter, MACHINE, SCOPE).unwrap()
}
const fn machine_head(revision: u64) -> MachineHead {
    MachineHead {
        scope: MACHINE,
        revision: MachineRevision::new(revision),
    }
}
fn timeline_head(step: u64, revision: u64) -> TimelineHead {
    TimelineHead::new(
        SCOPE,
        TimelineBranchId::ZERO,
        TimelineStep::new(step),
        TimelineRevision::new(revision),
        None,
    )
    .unwrap()
}
fn input(head: MachineHead, event: u64, identity: u64, time: u64) -> MachineTimelineInput<u64> {
    MachineTimelineInput {
        context: TransitionContext {
            expected: head,
            event: MachineEventId::from_bytes(Digest::of(&identity.to_le_bytes()).0),
            event_digest: event.digest().0,
            logical_time: MachineLogicalTime::new(time),
        },
        event,
    }
}
fn identity(value: u64) -> TimelineInputId {
    TimelineInputId::from_digest(Digest::of(&value.to_le_bytes()))
}
#[allow(clippy::unnecessary_wraps)] // CandidateValidator signature.
const fn valid(
    _state: &MachineTimelineState<u64>,
    _head: TimelineHead,
) -> Result<(), TimelineError> {
    Ok(())
}

#[test]
fn native_memory_and_timeline_agree_on_state_provenance_intents_and_failure_isolation() {
    for seed in 0..32 {
        let adapter = adapter();
        let mut native = MemoryMachine::<_, 128>::restore(
            Counter,
            machine_head(17),
            5,
            IntentLimits {
                max_intents: 128,
                max_payload_bytes: 1024,
            },
        )
        .unwrap();
        let mut live = Live::new(
            timeline_head(80, 700),
            adapter.restore(machine_head(17), 5).unwrap(),
            LIMITS,
        );
        for index in 0..64 {
            let event = (index * 13 + seed * 7) % 17;
            let input = input(native.read().0, event, index, seed + index);
            let before = (live.read().0, live.read().1.clone());
            let original = native.handle(&input.event, input.context);
            let candidate = live.apply_input(&adapter, identity(index), &input, &valid);
            match original {
                Ok(HandleOutcome::Accepted(record)) => {
                    candidate.unwrap();
                    assert_eq!(live.read().1.last_transition(), Some(record));
                }
                Ok(HandleOutcome::Rejected { reason }) => {
                    assert!(
                        matches!(candidate, Err(ApplyError::Computation(MachineTimelineError::Rejected { reason: actual })) if actual == reason)
                    );
                    assert_eq!(live.read(), (before.0, &before.1));
                }
                Ok(HandleOutcome::Unhandled) => {
                    assert!(matches!(
                        candidate,
                        Err(ApplyError::Computation(MachineTimelineError::Unhandled))
                    ));
                    assert_eq!(live.read(), (before.0, &before.1));
                }
                Err(MachineFailure::Model(expected)) => {
                    assert!(
                        matches!(candidate, Err(ApplyError::Computation(MachineTimelineError::Machine(MachineFailure::Model(actual)))) if actual == expected)
                    );
                    assert_eq!(live.read(), (before.0, &before.1));
                }
                Err(MachineFailure::Kernel(error)) => {
                    panic!("unexpected native failure: {error:?}")
                }
            }
            assert_eq!(live.read().1.read(), native.read());
            let mut observed: Vec<_> = live
                .tentative_effects()
                .map(|effect| {
                    (
                        effect.descriptor.identity.digest().0,
                        effect.descriptor.digest.0,
                        effect.descriptor.payload_bytes,
                        effect.payload.context,
                        effect.payload.intent.metadata,
                        effect.payload.intent.payload,
                    )
                })
                .collect();
            let mut expected: Vec<_> = native
                .pending_intents()
                .map(|intent| {
                    (
                        intent.intent.metadata.identity.bytes(),
                        intent.intent.metadata.digest,
                        intent.intent.metadata.payload_bytes,
                        intent.origin.context,
                        intent.intent.metadata,
                        intent.intent.payload,
                    )
                })
                .collect();
            observed.sort_by_key(|entry| entry.0);
            expected.sort_by_key(|entry| entry.0);
            assert_eq!(observed, expected);
            assert_eq!(live.eligible_effects().count(), 0);
        }
    }
}

#[test]
fn corrected_replay_uses_captured_context_and_existing_finality_and_handoff() {
    let adapter = adapter();
    let initial = adapter.restore(machine_head(0), 0).unwrap();
    let genesis = timeline_head(0, 0);
    let mut live = Live::new(genesis, initial.clone(), LIMITS);
    let mut retained = Vec::new();
    for index in 0..3 {
        let input = input(live.read().1.read().0, 8, index, 10 + index);
        let record = live
            .apply_input(&adapter, identity(index), &input, &valid)
            .unwrap();
        retained.push(CapturedInput::new(record, input).unwrap());
    }
    let predicted = (live.read().0, live.read().1.clone());
    assert_eq!(*predicted.1.read().1, 57);
    let server = adapter.restore(machine_head(0), 100).unwrap();
    let snapshot = CapturedSnapshot::capture(
        predicted.0,
        TimelineStep::ZERO,
        TimelineSnapshotId::from_digest(server.digest()),
        server,
    )
    .unwrap();
    let mut history = FixedHistory::<_, MachineTimelineInput<u64>, 3, 1, 1>::new(
        snapshot.anchor(),
        adapter.restore(machine_head(0), 100).unwrap(),
        HistoryLimits {
            max_input_bytes: 192,
            max_snapshot_bytes: 321,
            max_total_bytes: 1024,
        },
    )
    .unwrap();
    for captured in &retained {
        let bound = captured.read(captured.record()).unwrap();
        history
            .append_input(bound.record, bound.value.clone())
            .unwrap();
    }
    let records: Vec<_> = retained.iter().map(CapturedInput::record).collect();
    let observation = AuthorityObservation {
        scope: SCOPE,
        authority: TimelineAuthorityId::from_digest(Digest([99; 32])),
        epoch: AuthorityEpoch::ZERO,
        sequence: AuthoritySequence::new(1),
        step: TimelineStep::ZERO,
        digest: snapshot.anchor().digest,
    };
    let correction = CorrectionRequest {
        correction: Correction::State(snapshot.anchor()),
        inputs: &records,
        observation,
        scratch: initial.clone(),
        limits: ReplayLimits {
            max_retained_inputs: 3,
            max_replay_steps: 3,
        },
    };
    let mut correction = live.begin_correction(correction, &history).unwrap();
    loop {
        let phase = correction.advance(&adapter, &history, 1).unwrap();
        assert_eq!(correction.read(), (predicted.0, &predicted.1));
        if phase != ReplayPhase::Replaying {
            break;
        }
    }
    let corrected_head = correction.publish(&valid).unwrap();
    assert_ne!(corrected_head.branch(), predicted.0.branch());
    assert_eq!(corrected_head.step(), predicted.0.step());
    assert!(corrected_head.revision() > predicted.0.revision());
    assert_eq!(live.read().1.read(), (machine_head(3), &157));
    assert_eq!(live.tentative_effects().count(), 3);
    assert_eq!(live.eligible_effects().count(), 0);
    for effect in live.tentative_effects() {
        let original = retained
            .iter()
            .map(|captured| captured.read(captured.record()).unwrap())
            .find(|bound| {
                bound.value.context.event.bytes() == effect.descriptor.identity.digest().0
            })
            .unwrap();
        assert_eq!(effect.payload.context, original.value.context);
        assert_eq!(effect.descriptor.origin.branch, corrected_head.branch());
    }
    live.finalize_effects(corrected_head, TimelineStep::new(1))
        .unwrap();
    assert_eq!(live.eligible_effects().count(), 1);
    struct Handoff(Vec<TransitionContext>);
    impl EffectHandoff<MachineTimelineIntent<u64>> for Handoff {
        type Error = TimelineError;
        fn submit(
            &mut self,
            effect: EligibleEffect<'_, MachineTimelineIntent<u64>>,
        ) -> Result<EffectAdmissionReceipt, Self::Error> {
            self.0.push(effect.payload().context);
            Ok(EffectAdmissionReceipt {
                descriptor: effect.descriptor(),
                durable_commit: Digest([8; 32]),
            })
        }
    }
    let mut handoff = Handoff(Vec::new());
    assert_eq!(live.handoff(&mut handoff, 128).unwrap(), 1);
    assert_eq!(
        handoff.0,
        vec![
            retained[0]
                .read(retained[0].record())
                .unwrap()
                .value
                .context
        ]
    );
    // The same correction now crosses the independently finalized frontier.
    let before = (live.read().0, live.read().1.clone());
    let refused = live.begin_correction(
        CorrectionRequest {
            correction: Correction::State(StateAnchor {
                branch: before.0.branch(),
                ..snapshot.anchor()
            }),
            inputs: &[],
            observation,
            scratch: initial,
            limits: ReplayLimits {
                max_retained_inputs: 3,
                max_replay_steps: 3,
            },
        },
        &snapshot,
    );
    assert!(matches!(
        refused,
        Err(RestoreError::Timeline(
            TimelineError::ReconciliationRequired
        ))
    ));
    drop(refused);
    assert_eq!(live.read(), (before.0, &before.1));
}

#[test]
fn scope_definition_stale_head_exhaustion_and_quota_failures_do_not_publish() {
    let adapter = adapter();
    let mut live = Live::new(
        timeline_head(0, 0),
        adapter.restore(machine_head(0), 0).unwrap(),
        LIMITS,
    );
    for variant in 0..4 {
        let mut input = input(machine_head(0), 8, 1, 0);
        match variant {
            0 => input.context.expected.scope.instance = MachineId::from_bytes([90; 32]),
            1 => input.context.expected.revision = MachineRevision::new(1),
            2 => input.context.expected.scope.definition.event_schema = 99,
            3 => input.context.expected.scope.tenant = MachineTenantId::from_bytes([90; 32]),
            _ => panic!("unexpected test variant"),
        }
        let before = (live.read().0, live.read().1.clone());
        assert!(
            live.apply_input(&adapter, identity(1), &input, &valid)
                .is_err()
        );
        assert_eq!(live.read(), (before.0, &before.1));
    }
    let mut other_scope = SCOPE;
    other_scope.model = TimelineModelId::from_digest(Digest([55; 32]));
    let wrong_domain = MachineTimeline::new(Counter, MACHINE, other_scope).unwrap();
    assert!(matches!(
        live.apply_input(
            &wrong_domain,
            identity(1),
            &input(machine_head(0), 8, 1, 0),
            &valid
        ),
        Err(ApplyError::Computation(MachineTimelineError::Timeline(
            TimelineError::ScopeMismatch
        )))
    ));
    assert_eq!(live.read().0, timeline_head(0, 0));
    assert_eq!(live.read().1.read(), (machine_head(0), &0));
    let mut changed = MACHINE;
    changed.definition.state_schema = 999;
    assert!(matches!(
        MachineTimeline::new(Counter, changed, SCOPE),
        Err(MachineError::DefinitionMismatch)
    ));
    assert!(adapter.restore(machine_head(0), u64::MAX).is_err());
    for (revision, limits) in [
        (u64::MAX, LIMITS),
        (
            0,
            EffectLimits {
                max_intents: 0,
                max_payload_bytes: 0,
            },
        ),
        (
            0,
            EffectLimits {
                max_intents: 128,
                max_payload_bytes: 7,
            },
        ),
    ] {
        let initial = adapter.restore(machine_head(revision), 0).unwrap();
        let mut live = Live::new(timeline_head(0, 0), initial.clone(), limits);
        assert!(
            live.apply_input(
                &adapter,
                identity(1),
                &input(machine_head(revision), 8, 1, 0),
                &valid
            )
            .is_err()
        );
        assert_eq!(live.read(), (timeline_head(0, 0), &initial));
        assert_eq!(live.tentative_effects().count(), 0);
    }
    let first = input(machine_head(0), 8, 1, 0);
    live.apply_input(&adapter, identity(1), &first, &valid)
        .unwrap();
    let before = (live.read().0, live.read().1.clone());
    // Different event/timeline input, but the same pending native intent ID.
    let duplicate = input(machine_head(1), 9, 1, 0);
    assert!(matches!(
        live.apply_input(&adapter, identity(2), &duplicate, &valid),
        Err(ApplyError::Timeline(TimelineError::InputMismatch))
    ));
    assert_eq!(live.read(), (before.0, &before.1));
}

#[test]
fn content_hash_binds_every_captured_context_role_and_application_payload() {
    let original = input(machine_head(0), 8, 1, 10);
    for variant in 0..11 {
        let mut changed = original.clone();
        match variant {
            0 => changed.context.expected.scope.tenant = MachineTenantId::from_bytes([88; 32]),
            1 => changed.context.expected.scope.instance = MachineId::from_bytes([88; 32]),
            2 => {
                changed.context.expected.scope.definition.fingerprint =
                    MachineDefinitionId::from_bytes([88; 32])
            }
            3 => changed.context.expected.scope.definition.state_schema += 1,
            4 => changed.context.expected.scope.definition.event_schema += 1,
            5 => changed.context.expected.revision = MachineRevision::new(1),
            6 => changed.context.event = MachineEventId::from_bytes([88; 32]),
            7 => changed.context.event_digest = [88; 32],
            8 => changed.context.logical_time = MachineLogicalTime::new(11),
            9 => changed.event += 1,
            10 => {
                changed.event += 1;
                changed.context.event_digest = changed.event.digest().0;
            }
            _ => panic!("unexpected test variant"),
        }
        assert_ne!(changed.digest(), original.digest());
        assert_eq!(changed.byte_len(), 192);
    }
    let adapter = adapter();
    let first = adapter.restore(machine_head(0), 0).unwrap();
    assert_ne!(
        first.digest(),
        adapter.restore(machine_head(1), 0).unwrap().digest()
    );
    assert_ne!(
        first.digest(),
        adapter.restore(machine_head(0), 1).unwrap().digest()
    );
    assert_eq!(first.byte_len(), 121);
    let mut left = Live::new(timeline_head(0, 0), first.clone(), LIMITS);
    let mut right = Live::new(timeline_head(0, 0), first, LIMITS);
    left.apply_input(
        &adapter,
        identity(1),
        &input(machine_head(0), 8, 1, 1),
        &valid,
    )
    .unwrap();
    right
        .apply_input(
            &adapter,
            identity(1),
            &input(machine_head(0), 9, 1, 0),
            &valid,
        )
        .unwrap();
    assert_eq!(left.read().1.read(), right.read().1.read());
    assert_ne!(left.read().1.digest(), right.read().1.digest());
    assert_eq!(left.read().1.byte_len(), 321);
}

#[test]
fn caught_rejected_payload_destructor_cannot_clear_poison_or_publish() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    struct Bomb;
    impl Drop for Bomb {
        fn drop(&mut self) {
            panic!("rejected payload destructor");
        }
    }
    struct Catching;
    impl Machine for Catching {
        type State = u64;
        type Event = u64;
        type Effect = Bomb;
        type Error = ();
        fn definition(&self) -> MachineDefinition {
            DEFINITION
        }
        fn validate(&self, _: &u64) -> Result<(), ()> {
            Ok(())
        }
        fn transition<S: IntentSink<Bomb>>(
            &self,
            _: &u64,
            _: &u64,
            _: &TransitionContext,
            sink: &mut S,
        ) -> Result<Transition<u64>, ()> {
            let caught = catch_unwind(AssertUnwindSafe(|| {
                sink.stage(ProposedIntent {
                    metadata: IntentMetadata {
                        identity: MachineIntentId::from_bytes([1; 32]),
                        digest: [2; 32],
                        payload_bytes: 1,
                    },
                    payload: Bomb,
                })
            }));
            assert!(caught.is_err());
            assert_eq!(sink.status(), Err(MachineError::StagingInterrupted));
            Ok(Transition::Accepted {
                next: 999,
                transition: TransitionId::new(1),
            })
        }
    }
    let adapter = MachineTimeline::new(Catching, MACHINE, SCOPE).unwrap();
    let initial = adapter.restore(machine_head(0), 0).unwrap();
    let mut live = LiveTimeline::<_, _, 1, 0>::new(
        timeline_head(0, 0),
        initial.clone(),
        EffectLimits {
            max_intents: 0,
            max_payload_bytes: 0,
        },
    );
    let result = live.apply_input(
        &adapter,
        identity(1),
        &input(machine_head(0), 8, 1, 0),
        &valid,
    );
    assert!(matches!(
        result,
        Err(ApplyError::Computation(MachineTimelineError::Machine(
            MachineFailure::Kernel(MachineError::StagingInterrupted)
        )))
    ));
    assert_eq!(live.read(), (timeline_head(0, 0), &initial));
    assert_eq!(live.tentative_effects().count(), 0);
}

#[test]
fn complete_non_clone_state_uses_existing_prepare_live_step_path() {
    struct NonClone(u64);
    struct Model;
    impl Machine for Model {
        type State = NonClone;
        type Event = u64;
        type Effect = ();
        type Error = ();
        fn definition(&self) -> MachineDefinition {
            DEFINITION
        }
        fn validate(&self, _: &NonClone) -> Result<(), ()> {
            Ok(())
        }
        fn transition<S: IntentSink<()>>(
            &self,
            current: &NonClone,
            event: &u64,
            _: &TransitionContext,
            _: &mut S,
        ) -> Result<Transition<NonClone>, ()> {
            Ok(Transition::Accepted {
                next: NonClone(current.0 + event),
                transition: TransitionId::new(1),
            })
        }
    }
    struct Snapshot {
        anchor: StateAnchor,
    }
    impl SnapshotStore<MachineTimelineState<NonClone>> for Snapshot {
        type Error = ();
        fn restore(
            &self,
            anchor: StateAnchor,
            scratch: &mut MachineTimelineState<NonClone>,
        ) -> Result<StateAnchor, ()> {
            if anchor != self.anchor {
                return Err(());
            }
            *scratch = MachineTimeline::new(Model, MACHINE, SCOPE)
                .unwrap()
                .restore(machine_head(0), NonClone(9))
                .unwrap();
            Ok(anchor)
        }
    }
    impl LiveSnapshotStore<MachineTimelineState<NonClone>> for Snapshot {
        fn restore_current(
            &self,
            expected: TimelineHead,
            anchor: StateAnchor,
            scratch: &mut MachineTimelineState<NonClone>,
        ) -> Result<StateAnchor, ()> {
            if expected != timeline_head(0, 0) {
                return Err(());
            }
            self.restore(anchor, scratch)
        }
    }
    let adapter = MachineTimeline::new(Model, MACHINE, SCOPE).unwrap();
    let mut live = LiveTimeline::<_, _, 1, 0>::new(
        timeline_head(0, 0),
        adapter.restore(machine_head(0), NonClone(9)).unwrap(),
        EffectLimits {
            max_intents: 0,
            max_payload_bytes: 0,
        },
    );
    let captured = input(machine_head(0), 8, 1, 0);
    let descriptor = InputRecord {
        scope: SCOPE,
        branch: TimelineBranchId::ZERO,
        step: TimelineStep::new(1),
        identity: identity(1),
        revision: InputRevision::ZERO,
        digest: captured.digest(),
    };
    let inputs = CapturedInput::new(descriptor, captured).unwrap();
    let snapshots = Snapshot {
        anchor: StateAnchor {
            scope: SCOPE,
            branch: TimelineBranchId::ZERO,
            step: TimelineStep::ZERO,
            snapshot: TimelineSnapshotId::from_digest(Digest([1; 32])),
            digest: Digest([2; 32]),
        },
    };
    let ready = prepare_live_step::<_, _, _, _, 0>(
        LiveStepRequest {
            expected: live.read().0,
            anchor: snapshots.anchor,
            input: descriptor,
            scratch: adapter.restore(machine_head(0), NonClone(1000)).unwrap(),
            limits: EffectLimits {
                max_intents: 0,
                max_payload_bytes: 0,
            },
        },
        &adapter,
        &inputs,
        &snapshots,
        &|_: &MachineTimelineState<NonClone>, _: TimelineHead| Ok(()),
    )
    .unwrap();
    assert_eq!(live.read().1.read().1.0, 9);
    live.append(ready).unwrap();
    assert_eq!(live.read().1.read().0, machine_head(1));
    assert_eq!(live.read().1.read().1.0, 17);
}

struct Tree;
impl HierarchyModel for Tree {
    type State = u64;
    type Event = u64;
    type Effect = u64;
    type Error = MachineError;
    type Node = u64;
    fn definition(&self) -> MachineDefinition {
        DEFINITION
    }
    fn leaf(&self, state: &u64) -> u64 {
        state % 2
    }
    fn parent(&self, node: u64) -> Option<u64> {
        if node == 2 { None } else { Some(2) }
    }
    fn validate(&self, _: &u64) -> Result<(), Self::Error> {
        Ok(())
    }
    fn select(
        &self,
        node: u64,
        current: &u64,
        _: &u64,
        _: &TransitionContext,
    ) -> Result<HierarchyDecision<u64>, Self::Error> {
        if node != 2 {
            return Ok(HierarchyDecision::Unhandled);
        }
        Ok(HierarchyDecision::Selected(HierarchyPlan {
            target: 1 - current % 2,
            kind: TransitionKind::External,
            transition: TransitionId::new(18),
        }))
    }
    fn candidate(
        &self,
        _: HierarchyPlan<u64>,
        current: &u64,
        _: &u64,
        _: &TransitionContext,
    ) -> Result<u64, Self::Error> {
        Ok(current + 1)
    }
    fn exit<S: IntentSink<u64>>(
        &self,
        node: u64,
        _: &u64,
        _: &mut u64,
        context: &TransitionContext,
        sink: &mut S,
    ) -> Result<(), Self::Error> {
        hook(10 + node, context, sink)
    }
    fn action<S: IntentSink<u64>>(
        &self,
        _: HierarchyPlan<u64>,
        _: &u64,
        _: &mut u64,
        context: &TransitionContext,
        sink: &mut S,
    ) -> Result<(), Self::Error> {
        hook(20, context, sink)
    }
    fn enter<S: IntentSink<u64>>(
        &self,
        node: u64,
        _: &u64,
        _: &mut u64,
        context: &TransitionContext,
        sink: &mut S,
    ) -> Result<(), Self::Error> {
        hook(30 + node, context, sink)
    }
}
fn hook(
    value: u64,
    context: &TransitionContext,
    sink: &mut impl IntentSink<u64>,
) -> Result<(), MachineError> {
    let mut bytes = context.event.bytes();
    bytes[0] = u8::try_from(value).unwrap();
    sink.stage(ProposedIntent {
        metadata: IntentMetadata {
            identity: MachineIntentId::from_bytes(bytes),
            digest: value.digest().0,
            payload_bytes: 8,
        },
        payload: value,
    })
}

#[test]
fn hierarchical_correction_reuses_native_hooks_and_stops_on_ignored_capacity_failure() {
    let adapter = MachineTimeline::new(Hierarchical::<_, 2>::new(Tree), MACHINE, SCOPE).unwrap();
    let initial = adapter.restore(machine_head(0), 0).unwrap();
    let mut live = Live::new(timeline_head(0, 0), initial.clone(), LIMITS);
    let mut retained = Vec::new();
    for index in 0..2 {
        let input = input(machine_head(index), 8, index, index);
        let record = live
            .apply_input(&adapter, identity(index), &input, &valid)
            .unwrap();
        retained.push(CapturedInput::new(record, input).unwrap());
    }
    let baseline = live.read().0;
    let corrected = adapter.restore(machine_head(0), 100).unwrap();
    let snapshot = CapturedSnapshot::capture(
        baseline,
        TimelineStep::ZERO,
        TimelineSnapshotId::from_digest(corrected.digest()),
        corrected,
    )
    .unwrap();
    let records: Vec<_> = retained.iter().map(CapturedInput::record).collect();
    let mut correction = live
        .begin_correction(
            CorrectionRequest {
                correction: Correction::State(snapshot.anchor()),
                inputs: &records,
                observation: AuthorityObservation {
                    scope: SCOPE,
                    authority: TimelineAuthorityId::from_digest(Digest([7; 32])),
                    epoch: AuthorityEpoch::ZERO,
                    sequence: AuthoritySequence::new(1),
                    step: TimelineStep::ZERO,
                    digest: snapshot.anchor().digest,
                },
                scratch: initial,
                limits: ReplayLimits {
                    max_retained_inputs: 2,
                    max_replay_steps: 2,
                },
            },
            &snapshot,
        )
        .unwrap();
    assert_eq!(
        correction
            .advance(&adapter, retained.as_slice(), 2)
            .unwrap(),
        ReplayPhase::ReadyToValidate
    );
    correction.publish(&valid).unwrap();
    assert_eq!(live.read().1.read(), (machine_head(2), &102));
    assert_eq!(
        live.read().1.last_transition().unwrap().transition,
        TransitionId::new(18)
    );
    let mut effects: Vec<_> = live
        .tentative_effects()
        .map(|effect| {
            (
                effect.descriptor.origin.step.get(),
                effect.payload.intent.payload,
            )
        })
        .collect();
    effects.sort_unstable();
    assert_eq!(
        effects,
        vec![(1, 10), (1, 20), (1, 31), (2, 11), (2, 20), (2, 30)]
    );
    // Only enough room for exit + action: entry failure discards the whole step.
    let initial = adapter.restore(machine_head(0), 0).unwrap();
    let mut short = Live::new(
        timeline_head(0, 0),
        initial.clone(),
        EffectLimits {
            max_intents: 2,
            max_payload_bytes: 16,
        },
    );
    assert!(
        short
            .apply_input(
                &adapter,
                identity(1),
                &input(machine_head(0), 8, 1, 0),
                &valid
            )
            .is_err()
    );
    assert_eq!(short.read(), (timeline_head(0, 0), &initial));
    assert_eq!(short.tentative_effects().count(), 0);
}
