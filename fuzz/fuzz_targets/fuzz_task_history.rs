#![no_main]
use libfuzzer_sys::fuzz_target;
use penelope_ports::task::*;

fuzz_target!(|data: &[u8]| {
    let definition = TaskDefinition {
        code: Digest([3; 32]),
        checkpoint_schema: 1,
        effects: if data.first().is_some_and(|b| b & 1 == 1) {
            EffectContract::ReconcileRequired
        } else {
            EffectContract::Pure
        },
        recovery: TaskRecoveryPolicy {
            mode: match data.first().copied().unwrap_or(0) >> 1 & 3 {
                0 => RecoveryMode::RestartFromBeginning,
                1 => RecoveryMode::RestartFromCheckpoint,
                2 => RecoveryMode::ReconcileBeforeRestart,
                _ => RecoveryMode::NeverRestart,
            },
            max_attempts: u32::from(data.last().copied().unwrap_or(0) % 8) + 1,
            timeout_ticks: u64::from(data.first().copied().unwrap_or(0) % 15) + 1,
            restart_delay_ticks: 1,
            max_checkpoint_bytes: 64,
        },
        budget: ExecutionBudget {
            max_input_bytes: 64,
            max_output_bytes: 64,
            max_history_records: u32::from(data.last().copied().unwrap_or(0) % 32).max(4),
            max_checkpoint_commits: u32::from(data.last().copied().unwrap_or(0) % 8),
            cpu_ticks: 10,
            deadline_tick: 1000,
        },
    };
    let scope = TaskScope {
        tenant: Digest([1; 32]),
        task: Digest([2; 32]),
        definition: definition.fingerprint(),
        input: Digest([4; 32]),
    };
    let mut projection = TaskProjection::new(scope, definition).unwrap();
    let mut accepted = Vec::new();
    let input = BlobRef {
        digest: scope.input,
        bytes: 1,
        schema: 1,
    };
    let intent = projection
        .propose(
            TaskExecutionEvent::IntentCommitted { definition, input },
            0,
            0,
        )
        .unwrap();
    projection.apply(&intent).unwrap();
    accepted.push(intent);
    // Malformed wire must not mutate an existing valid projection.
    if let Ok(record) = serde_json::from_slice::<TaskRecord>(data) {
        let before = projection;
        if projection.apply(&record).is_err() {
            assert_eq!(projection, before);
        }
        projection = before;
    }
    for (index, chunk) in data.chunks(4).take(100).enumerate() {
        let byte = |position: usize| chunk.get(position).copied().unwrap_or(0);
        let tick = index as u64 + 1;
        let checkpoint = BlobRef {
            digest: Digest([byte(2); 32]),
            bytes: u32::from(byte(3)),
            schema: u32::from(byte(2) % 3),
        };
        let event = match byte(0) % 12 {
            0 => TaskExecutionEvent::Started,
            1 => TaskExecutionEvent::Restarted,
            2 => TaskExecutionEvent::CheckpointCommitted { checkpoint },
            3 => TaskExecutionEvent::FaultObserved {
                fault: TaskFault::Crash,
            },
            4 => TaskExecutionEvent::ReconciliationRequired,
            5 => TaskExecutionEvent::Reconciled {
                result: Reconciliation::Unknown {
                    evidence: checkpoint.digest,
                },
            },
            6 => TaskExecutionEvent::Reconciled {
                result: Reconciliation::RestartAuthorized {
                    evidence: checkpoint.digest,
                },
            },
            7 => TaskExecutionEvent::RecoveryPlanned {
                resume: projection.checkpoint(),
            },
            8 => TaskExecutionEvent::Completed { output: checkpoint },
            9 => TaskExecutionEvent::RecoveryExhausted,
            10 => TaskExecutionEvent::FaultObserved {
                fault: TaskFault::Timeout,
            },
            _ => TaskExecutionEvent::Reconciled {
                result: Reconciliation::Completed {
                    output: checkpoint,
                    evidence: checkpoint.digest,
                },
            },
        };
        let generation = if matches!(
            event,
            TaskExecutionEvent::Started | TaskExecutionEvent::Restarted
        ) {
            projection.generation().saturating_add(1)
        } else {
            projection.generation()
        };
        let record = TaskRecord {
            version: if byte(1) & 1 == 0 { 1 } else { 2 },
            scope,
            sequence: projection.next_sequence(),
            generation: if byte(1) & 2 == 0 {
                generation
            } else {
                generation.saturating_sub(1)
            },
            tick,
            event,
        };
        let before = projection;
        match projection.apply(&record) {
            Ok(()) => {
                accepted.push(record);
                assert_eq!(
                    replay(scope, definition, accepted.clone()).unwrap(),
                    projection
                );
            }
            Err(_) => assert_eq!(projection, before),
        }
        assert!(projection.generation() <= definition.recovery.max_attempts);
    }
});
