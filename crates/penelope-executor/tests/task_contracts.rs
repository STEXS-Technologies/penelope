#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::shadow_unrelated
)]
#[path = "support/tasks.rs"]
mod support;
use penelope_core::task::*;
use penelope_executor::task::TaskSupervisor;
use support::*;

fn pending() -> TaskProjection {
    let definition = definition(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    let scope = scope(definition, &[1; 64]);
    let mut projection = TaskProjection::new(scope, definition).unwrap();
    let record = projection
        .propose(
            TaskExecutionEvent::IntentCommitted {
                definition,
                input: BlobRef {
                    digest: scope.input,
                    bytes: 64,
                    schema: 1,
                },
            },
            0,
            0,
        )
        .unwrap();
    projection.apply(&record).unwrap();
    projection
}

#[test]
fn malformed_history_is_rejected_without_mutating_projection() {
    let pending = pending();
    let start = pending.next(1).unwrap().unwrap();
    for mut record in [start; 5]
        .into_iter()
        .enumerate()
        .map(|(index, mut record)| {
            match index {
                0 => record.version = 2,
                1 => record.sequence = 9,
                2 => record.scope.task = Digest([9; 32]),
                3 => record.generation = 0,
                _ => record.event = TaskExecutionEvent::RecoveryExhausted,
            }
            record
        })
    {
        let mut projection = pending;
        assert!(projection.apply(&record).is_err());
        assert_eq!(projection, pending);
        record.tick = u64::MAX;
        assert!(projection.apply(&record).is_err());
        assert_eq!(projection, pending);
    }
    let mut projection = pending;
    projection.apply(&start).unwrap();
    assert_eq!(projection.next(0), Err(TaskError::InvalidOrder));
    let checkpoint = BlobRef {
        digest: Digest([7; 32]),
        bytes: 12,
        schema: 2,
    };
    assert_eq!(
        projection.propose(TaskExecutionEvent::CheckpointCommitted { checkpoint }, 1, 2),
        Err(TaskError::SchemaMismatch)
    );
}

#[test]
fn checkpoint_upgrade_requires_explicit_application_authorized_import() {
    let old = pending();
    let mut upgraded = old.definition();
    upgraded.code = Digest([9; 32]);
    upgraded.checkpoint_schema = 2;
    assert!(replay(old.scope(), upgraded, []).is_err());
    let new_scope = TaskScope {
        definition: upgraded.code,
        ..old.scope()
    };
    let mut supervisor = TaskSupervisor::<_, _, 64>::restore(
        MemoryJournal::default(),
        SumRuntime::default(),
        new_scope,
        upgraded,
        0,
    )
    .unwrap();
    supervisor.submit(&[1; 64], 1, 0).unwrap();
    let previous = BlobRef {
        digest: Digest([8; 32]),
        bytes: 12,
        schema: 1,
    };
    let transformed = [8, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0];
    supervisor
        .import_checkpoint(old.scope(), previous, &transformed, Digest([5; 32]), 1)
        .unwrap();
    assert_eq!(supervisor.projection().checkpoint().unwrap().schema, 2);
    supervisor.advance(2).unwrap();
    assert_eq!(supervisor.into_parts().1.resumed, [8]);
}

#[test]
fn quotas_and_deadline_are_checked_before_new_execution() {
    let mut definition = definition(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    definition.budget.max_checkpoint_commits = 0;
    let scope = scope(definition, &[1; 64]);
    let mut supervisor = TaskSupervisor::<_, _, 64>::restore(
        MemoryJournal::default(),
        SumRuntime::default(),
        scope,
        definition,
        0,
    )
    .unwrap();
    supervisor.submit(&[1; 64], 1, 0).unwrap();
    supervisor.advance(1).unwrap();
    assert!(supervisor.poll(2).is_err());
    let mut projection = pending();
    let terminal = projection
        .next(projection.definition().budget.deadline_tick)
        .unwrap()
        .unwrap();
    projection.apply(&terminal).unwrap();
    assert_eq!(projection.status(), TaskStatus::Exhausted);
    definition.budget.max_input_bytes = 65;
    assert!(
        TaskSupervisor::<_, _, 64>::restore(
            MemoryJournal::default(),
            SumRuntime::default(),
            scope,
            definition,
            0
        )
        .is_err()
    );
}

#[test]
fn task_wire_v1_fixture_is_immutable() {
    let record: TaskRecord =
        serde_json::from_str(include_str!("fixtures/task-start-v1.json")).unwrap();
    assert_eq!(record.version, 1);
    assert_eq!(record.sequence, 1);
    assert_eq!(record.generation, 1);
    assert_eq!(record.event, TaskExecutionEvent::Started);
    assert_eq!(
        serde_json::to_value(record).unwrap(),
        serde_json::from_str::<serde_json::Value>(include_str!("fixtures/task-start-v1.json"))
            .unwrap()
    );
}
