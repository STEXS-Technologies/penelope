//! Baseline-qualified downstream task/timeline protocols and exhaustive matches.
use penelope::core::{task::*, timeline::*};
use std::error::Error;

fn status(value: TaskStatus) -> &'static str {
    match value {
        TaskStatus::Empty => "empty",
        TaskStatus::Pending => "pending",
        TaskStatus::Running => "running",
        TaskStatus::Faulted => "faulted",
        TaskStatus::Reconciling => "reconciling",
        TaskStatus::Reconciled => "reconciled",
        TaskStatus::Ready => "ready",
        TaskStatus::Completed => "completed",
        TaskStatus::Exhausted => "exhausted",
    }
}
fn definition(mode: RecoveryMode, effects: EffectContract) -> TaskDefinition {
    TaskDefinition {
        code: Digest([3; 32]),
        checkpoint_schema: 1,
        effects,
        recovery: TaskRecoveryPolicy {
            mode,
            max_attempts: 2,
            timeout_ticks: 20,
            restart_delay_ticks: 1,
            max_checkpoint_bytes: 8,
        },
        budget: ExecutionBudget {
            max_input_bytes: 8,
            max_output_bytes: 8,
            max_history_records: 32,
            max_checkpoint_commits: 4,
            cpu_ticks: 10,
            deadline_tick: TaskTick::new(1000),
        },
    }
}
fn tasks() -> Result<(), Box<dyn Error>> {
    // Frozen function-pointer types also detect downstream signature drift.
    let _: fn(TaskScope, TaskDefinition) -> Result<TaskProjection, TaskError> = TaskProjection::new;
    let _: fn(&mut TaskProjection, &TaskRecord) -> Result<(), TaskError> = TaskProjection::apply;
    let _: fn(
        &TaskProjection,
        TaskExecutionEvent,
        TaskGeneration,
        TaskTick,
    ) -> Result<TaskRecord, TaskError> = TaskProjection::propose;
    for mode in [
        RecoveryMode::RestartFromBeginning,
        RecoveryMode::RestartFromCheckpoint,
        RecoveryMode::ReconcileBeforeRestart,
        RecoveryMode::NeverRestart,
    ] {
        for effects in [EffectContract::Pure, EffectContract::ReconcileRequired] {
            let definition = definition(mode, effects);
            let input = BlobRef {
                digest: Digest::of(&[1; 8]),
                bytes: 8,
                schema: 1,
            };
            let scope = TaskScope {
                tenant: TaskTenantId::from_digest(Digest([1; 32])),
                task: TaskId::from_digest(Digest([2; 32])),
                definition: definition.fingerprint(),
                input: TaskInputDigest::from_digest(input.digest),
            };
            let checkpoint = BlobRef {
                digest: Digest::of(&[4; 8]),
                bytes: 8,
                schema: 1,
            };
            let output = BlobRef {
                digest: Digest::of(&[5; 8]),
                bytes: 8,
                schema: 1,
            };
            let mut old = definition;
            old.code = Digest([6; 32]);
            let mut old_scope = scope;
            old_scope.definition = old.fingerprint();
            let mut events = vec![
                TaskExecutionEvent::IntentCommitted { definition, input },
                TaskExecutionEvent::Started,
                TaskExecutionEvent::CheckpointCommitted { checkpoint },
                TaskExecutionEvent::FaultObserved {
                    fault: TaskFault::Crash,
                },
                TaskExecutionEvent::ReconciliationRequired,
                TaskExecutionEvent::Reconciled {
                    result: Reconciliation::Unknown {
                        evidence: Digest([7; 32]),
                    },
                },
                TaskExecutionEvent::Reconciled {
                    result: Reconciliation::RestartAuthorized {
                        evidence: Digest([8; 32]),
                    },
                },
                TaskExecutionEvent::RecoveryPlanned { resume: None },
                TaskExecutionEvent::RecoveryPlanned {
                    resume: Some(checkpoint),
                },
                TaskExecutionEvent::Restarted,
                TaskExecutionEvent::Completed { output },
                TaskExecutionEvent::RecoveryExhausted,
                TaskExecutionEvent::Reconciled {
                    result: Reconciliation::Completed {
                        output,
                        evidence: Digest([9; 32]),
                    },
                },
                TaskExecutionEvent::CheckpointMigrated {
                    source: old_scope,
                    previous: checkpoint,
                    checkpoint,
                    authorization: Digest([10; 32]),
                },
            ];
            for fault in [
                TaskFault::ProtectionViolation,
                TaskFault::WatchdogExpired,
                TaskFault::Timeout,
                TaskFault::DeadlineViolation,
                TaskFault::CpuBudgetExceeded,
                TaskFault::SupervisorInterrupted,
                TaskFault::ApplicationFailure,
            ] {
                events.push(TaskExecutionEvent::FaultObserved { fault });
            }
            for branch in 0..3 {
                let mut projection = TaskProjection::new(scope, definition)?;
                let mut history = Vec::new();
                let path = if branch == 0 {
                    vec![0, 1, 2, 10]
                } else {
                    vec![
                        0,
                        13,
                        1,
                        2,
                        3,
                        4,
                        5,
                        if branch == 1 { 6 } else { 12 },
                        7,
                        8,
                        9,
                        10,
                        11,
                    ]
                };
                for (step, index) in path.into_iter().enumerate() {
                    let tick = TaskTick::new(u64::try_from(step)? + 1);
                    println!(
                        "task {mode:?} {effects:?} {branch} {step} {} {:?} {:?}",
                        status(projection.status()),
                        projection.checkpoint(),
                        projection.output()
                    );
                    // Complete event variant shapes and validation/error precedence.
                    for (probe, event) in events.iter().copied().enumerate() {
                        let generation = if matches!(
                            event,
                            TaskExecutionEvent::Started | TaskExecutionEvent::Restarted
                        ) {
                            TaskGeneration::new(projection.generation().get() + 1)
                        } else {
                            projection.generation()
                        };
                        let raw = TaskRecord {
                            version: 1,
                            scope,
                            sequence: projection.next_sequence(),
                            generation,
                            tick,
                            event,
                        };
                        let encoded = serde_json::to_string(&raw)?;
                        let decoded: TaskRecord = serde_json::from_str(&encoded)?;
                        assert_eq!(raw, decoded);
                        let result = projection.propose(event, generation, tick);
                        let tag = match event {
                            TaskExecutionEvent::IntentCommitted { .. } => "intent",
                            TaskExecutionEvent::CheckpointMigrated { .. } => "migration",
                            TaskExecutionEvent::Started => "start",
                            TaskExecutionEvent::CheckpointCommitted { .. } => "checkpoint",
                            TaskExecutionEvent::FaultObserved { .. } => "fault",
                            TaskExecutionEvent::ReconciliationRequired => "reconcile",
                            TaskExecutionEvent::Reconciled { .. } => "evidence",
                            TaskExecutionEvent::RecoveryPlanned { .. } => "plan",
                            TaskExecutionEvent::Restarted => "restart",
                            TaskExecutionEvent::Completed { .. } => "complete",
                            TaskExecutionEvent::RecoveryExhausted => "exhausted",
                        };
                        println!("probe {probe} {tag} {encoded} {result:?}");
                        for defect in 0..5 {
                            let mut invalid = raw;
                            match defect {
                                0 => invalid.version = 2,
                                1 => invalid.scope = old_scope,
                                2 => invalid.sequence = TaskSequence::new(999),
                                3 => invalid.generation = TaskGeneration::new(999),
                                _ => invalid.tick = TaskTick::ZERO,
                            }
                            let mut candidate = projection;
                            let result = candidate.apply(&invalid);
                            if result.is_err() {
                                assert_eq!(candidate, projection);
                            }
                            println!("defect {defect} {result:?}");
                        }
                    }
                    let event = events[index];
                    let generation = if matches!(
                        event,
                        TaskExecutionEvent::Started | TaskExecutionEvent::Restarted
                    ) {
                        TaskGeneration::new(projection.generation().get() + 1)
                    } else {
                        projection.generation()
                    };
                    if let Ok(record) = projection.propose(event, generation, tick) {
                        projection.apply(&record)?;
                        history.push(record);
                        let mut replay = TaskProjection::new(scope, definition)?;
                        for accepted in &history {
                            replay
                                .apply(&serde_json::from_str(&serde_json::to_string(accepted)?)?)?;
                        }
                        assert_eq!(replay, projection);
                    }
                }
                println!("terminal {} {:?}", status(projection.status()), projection);
            }
        }
    }
    Ok(())
}
fn timelines() -> Result<(), Box<dyn Error>> {
    let scope = TimelineScope {
        tenant: TimelineTenantId::from_digest(Digest([1; 32])),
        timeline: TimelineId::from_digest(Digest([2; 32])),
        model: TimelineModelId::from_digest(Digest([3; 32])),
        schema: TimelineSchemaId::from_digest(Digest([4; 32])),
    };
    let head = TimelineHead::new(
        scope,
        TimelineBranchId::new(7),
        TimelineStep::new(6),
        TimelineRevision::new(8),
        Some(TimelineStep::new(1)),
    )?;
    let observation = AuthorityObservation {
        scope,
        authority: TimelineAuthorityId::from_digest(Digest([10; 32])),
        epoch: AuthorityEpoch::new(2),
        sequence: AuthoritySequence::new(3),
        step: TimelineStep::new(3),
        digest: Digest([9; 32]),
    };
    let mut projection = TimelineProjection::<1>::new(head);
    let observed = projection.observe(head, observation)?;
    println!(
        "observe {observed:?} duplicate={:?} stale={:?}",
        projection.observe(observed, observation),
        projection.observe(head, observation)
    );
    let input = InputRecord {
        scope,
        branch: TimelineBranchId::new(7),
        step: TimelineStep::new(4),
        identity: TimelineInputId::from_digest(Digest([5; 32])),
        revision: InputRevision::new(2),
        digest: Digest([6; 32]),
    };
    let anchor = StateAnchor {
        scope,
        branch: TimelineBranchId::new(7),
        step: TimelineStep::new(3),
        snapshot: TimelineSnapshotId::from_digest(Digest([8; 32])),
        digest: Digest([9; 32]),
    };
    let records = [
        TimelineRecord::Checkpoint {
            head,
            last_branch: TimelineBranchId::new(9),
        },
        TimelineRecord::Checkpoint {
            head: observed,
            last_branch: TimelineBranchId::new(9),
        },
        TimelineRecord::Input(input),
        TimelineRecord::Anchor(anchor),
        TimelineRecord::Fork {
            expected: observed,
            branch: TimelineBranchId::new(8),
            anchor,
            replacement: None,
        },
        TimelineRecord::Fork {
            expected: observed,
            branch: TimelineBranchId::new(8),
            anchor,
            replacement: Some(input),
        },
        TimelineRecord::Intent(TimelineIntentRecord {
            scope,
            branch: TimelineBranchId::new(8),
            step: TimelineStep::new(4),
            revision: TimelineRevision::new(10),
            identity: TimelineEffectId::from_digest(Digest([11; 32])),
            digest: Digest([12; 32]),
            payload_bytes: 12,
        }),
    ];
    for record in records {
        // Exhaustive matching is part of the downstream source contract.
        let kind = match record {
            TimelineRecord::Checkpoint { .. } => "checkpoint",
            TimelineRecord::Input(_) => "input",
            TimelineRecord::Anchor(_) => "anchor",
            TimelineRecord::Fork { .. } => "fork",
            TimelineRecord::Intent(_) => "intent",
        };
        let mut bytes = [0; TIMELINE_RECORD_MAX_BYTES + 1];
        let len = record.encode(&mut bytes)?;
        assert_eq!(TimelineRecord::decode(&bytes[..len])?, record);
        println!(
            "timeline {kind} {}",
            bytes[..len]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        for end in 0..len {
            println!(
                "truncated {end} {:?}",
                TimelineRecord::decode(&bytes[..end])
            );
        }
        println!("trailing {:?}", TimelineRecord::decode(&bytes[..len + 1]));
        if let TimelineRecord::Checkpoint { head, last_branch } = record {
            let restored = TimelineProjection::<1>::restore(head, last_branch)?;
            println!("restore {:?} {:?}", restored.head(), restored.last_branch());
        }
    }
    Ok(())
}
fn main() -> Result<(), Box<dyn Error>> {
    tasks()?;
    timelines()
}
