//! Candidate isolation and differential behavior against the exact 0.4 projection.
#![allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]
use penelope_core::{machine::PreparedDecision, task::*};
#[allow(dead_code)]
#[path = "support/task_projection_0_4.rs"]
mod legacy;

const fn definition(mode: RecoveryMode, effects: EffectContract) -> TaskDefinition {
    TaskDefinition {
        code: Digest([3; 32]),
        checkpoint_schema: 1,
        effects,
        recovery: TaskRecoveryPolicy {
            mode,
            max_attempts: 4,
            timeout_ticks: 12,
            restart_delay_ticks: 1,
            max_checkpoint_bytes: 8,
        },
        budget: ExecutionBudget {
            max_input_bytes: 8,
            max_output_bytes: 8,
            max_history_records: 64,
            max_checkpoint_commits: 8,
            cpu_ticks: 10,
            deadline_tick: TaskTick::new(1000),
        },
    }
}
fn scope(definition: TaskDefinition) -> TaskScope {
    TaskScope {
        tenant: TaskTenantId::from_digest(Digest([1; 32])),
        task: TaskId::from_digest(Digest([2; 32])),
        definition: definition.fingerprint(),
        input: TaskInputDigest::from_digest(Digest::of(&[7; 8])),
    }
}
fn assert_state(current: &TaskProjection, old: &legacy::TaskProjection) {
    // Debug includes all private fields, including recovery timing and commit counts.
    assert_eq!(format!("{current:?}"), format!("{old:?}"));
}
fn probe(current: &TaskProjection, old: &legacy::TaskProjection, record: TaskRecord) {
    let mut reference = *old;
    let expected = reference.apply(&record);
    let before = *current;
    match current.prepare(record) {
        Ok(ready) => {
            assert_eq!(expected, Ok(()));
            assert_eq!(ready.before(), Some(&before));
            assert_eq!(*ready.provenance(), record);
            assert_state(ready.candidate(), &reference);
            assert_eq!(ready.intents().count(), 0);
        }
        Err(error) => assert_eq!(expected, Err(error)),
    }
    assert_eq!(*current, before);
    let mut applied = *current;
    assert_eq!(applied.apply(&record), expected);
    assert_state(&applied, &reference);
}
fn commit(current: &mut TaskProjection, old: &mut legacy::TaskProjection, record: TaskRecord) {
    let prepared_record = current.prepare(record).unwrap().into_record();
    assert_eq!(prepared_record, record);
    assert_eq!(old.apply(&record), Ok(()));
    assert_eq!(current.apply(&prepared_record), Ok(()));
    assert_state(current, old);
}

#[test]
fn preparation_matches_frozen_record_validation_and_supervisory_decisions() {
    for mode in [
        RecoveryMode::RestartFromBeginning,
        RecoveryMode::RestartFromCheckpoint,
        RecoveryMode::ReconcileBeforeRestart,
        RecoveryMode::NeverRestart,
    ] {
        for effects in [EffectContract::Pure, EffectContract::ReconcileRequired] {
            for seed in 0_u64..8 {
                let definition = definition(mode, effects);
                let scope = scope(definition);
                let mut current = TaskProjection::new(scope, definition).unwrap();
                let mut old = legacy::TaskProjection::new(scope, definition).unwrap();
                let input = BlobRef {
                    digest: scope.input.digest(),
                    bytes: 8,
                    schema: 1,
                };
                let intent = current.propose(
                    TaskExecutionEvent::IntentCommitted { definition, input },
                    TaskGeneration::ZERO,
                    TaskTick::ZERO,
                );
                assert_eq!(
                    intent,
                    old.propose(
                        TaskExecutionEvent::IntentCommitted { definition, input },
                        TaskGeneration::ZERO,
                        TaskTick::ZERO
                    )
                );
                commit(&mut current, &mut old, intent.unwrap());
                let mut history = vec![intent.unwrap()];
                for step in 1_u64..80 {
                    let tick = TaskTick::new(step);
                    assert_eq!(current.next(tick), old.next(tick));
                    let blob = BlobRef {
                        digest: Digest([9; 32]),
                        bytes: 8,
                        schema: 1,
                    };
                    let event = match (step + seed) % 6 {
                        0 => TaskExecutionEvent::CheckpointCommitted { checkpoint: blob },
                        1 => TaskExecutionEvent::FaultObserved {
                            fault: TaskFault::Crash,
                        },
                        2 => TaskExecutionEvent::Reconciled {
                            result: Reconciliation::Unknown {
                                evidence: Digest([5; 32]),
                            },
                        },
                        3 => TaskExecutionEvent::Reconciled {
                            result: Reconciliation::RestartAuthorized {
                                evidence: Digest([6; 32]),
                            },
                        },
                        4 => TaskExecutionEvent::Completed { output: blob },
                        _ => TaskExecutionEvent::RecoveryExhausted,
                    };
                    assert_eq!(
                        current.propose(event, current.generation(), tick),
                        old.propose(event, old.generation(), tick)
                    );
                    let record = TaskRecord {
                        version: 1,
                        scope,
                        sequence: current.next_sequence(),
                        generation: current.generation(),
                        tick,
                        event,
                    };
                    probe(&current, &old, record);
                    for mutation in 0..6 {
                        let mut malformed = record;
                        match mutation {
                            0 => malformed.version = 2,
                            1 => malformed.scope.task = TaskId::from_digest(Digest([99; 32])),
                            2 => malformed.sequence = TaskSequence::new(u32::MAX),
                            3 => malformed.generation = TaskGeneration::new(u32::MAX),
                            4 => malformed.tick = TaskTick::ZERO,
                            _ => malformed.tick = TaskTick::new(u64::MAX),
                        }
                        probe(&current, &old, malformed);
                    }
                    let proposal = current
                        .next(tick)
                        .unwrap()
                        .or_else(|| current.propose(event, current.generation(), tick).ok());
                    if let Some(planned) = proposal {
                        // Some preparations are deliberately abandoned before retrying.
                        if (step + seed) % 7 != 0 {
                            commit(&mut current, &mut old, planned);
                            history.push(planned);
                        }
                    }
                }
                assert_state(
                    &replay(scope, definition, history.clone()).unwrap(),
                    &legacy::replay(scope, definition, history).unwrap(),
                );
            }
        }
    }
}

#[test]
fn frozen_reference_matches_the_released_source_bytes() {
    assert_eq!(
        Digest::of(include_bytes!("support/task_projection_0_4.rs")).0,
        [
            190, 44, 136, 231, 90, 190, 102, 124, 231, 127, 67, 123, 124, 245, 120, 98, 185, 144,
            226, 238, 195, 71, 120, 245, 69, 167, 15, 158, 177, 202, 186, 88
        ]
    );
}
