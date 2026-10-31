//! Stop confirmation cannot acknowledge a generation that fencing forbids.
use super::*;
#[test]
fn stop_confirmation_rejects_out_of_policy_rows_without_mutation() {
    let execution = Digest([9; 32]);
    for generation in [definition().recovery.max_attempts + 1, u32::MAX, 0] {
        for state in [1, 2] {
            for identity in [None, Some(execution)] {
                let directory = tempfile::tempdir().unwrap();
                let path = directory.path().join("confirm-bounds.db");
                let mut journal = open(&path);
                let projection = committed_start(&mut journal);
                journal
                    .begin_fence(scope(), projection.generation())
                    .unwrap();
                let connection = Connection::open(&path).unwrap();
                if generation == 0 {
                    connection
                        .execute_batch("PRAGMA ignore_check_constraints=ON")
                        .unwrap();
                }
                connection
                    .execute(
                        "UPDATE runtime_generations SET generation=?1,execution=?2,state=?3",
                        rusqlite::params![
                            generation,
                            identity.as_ref().map(|id| id.0.as_slice()),
                            state
                        ],
                    )
                    .unwrap();
                if generation == 0 {
                    connection
                        .execute_batch("PRAGMA ignore_check_constraints=OFF")
                        .unwrap();
                }
                let tables = [
                    "task_heads",
                    "task_records",
                    "task_blobs",
                    "runtime_generations",
                ];
                let before = write_snapshot::snapshot(&connection, &tables);
                assert_eq!(
                    journal.begin_fence(scope(), TaskGeneration::new(generation)),
                    Err(TaskPortError::Conflict)
                );
                assert_eq!(
                    journal.confirm_stopped(scope(), TaskGeneration::new(generation), identity),
                    Err(TaskPortError::Conflict)
                );
                assert_eq!(write_snapshot::snapshot(&connection, &tables), before);
                drop(journal);
                let mut reopened = open(&path);
                assert_eq!(
                    reopened.confirm_stopped(scope(), TaskGeneration::new(generation), identity),
                    Err(TaskPortError::Conflict)
                );
                assert_eq!(write_snapshot::snapshot(&connection, &tables), before);
                println!(
                    "generation={generation} state={state} reserved={} rejected_unchanged=true reopened=true",
                    identity.is_some()
                );
            }
        }
    }
}

#[test]
fn valid_confirmations_survive_last_attempt_retirement_and_reopen() {
    for reserved in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("confirm-retired.db");
        let mut journal = open(&path);
        let mut projection = committed_start(&mut journal);
        let execution = Digest([9; 32]);
        let identity = reserved.then_some(execution);
        for generation in 1..=definition().recovery.max_attempts {
            assert_eq!(projection.generation().get(), generation);
            if reserved {
                assert_eq!(
                    journal.reserve_start(request(&projection), execution),
                    Ok(StartReservation::Reserved)
                );
            }
            if generation == definition().recovery.max_attempts {
                let output = BlobRef {
                    digest: Digest::of(b"done"),
                    bytes: 4,
                    schema: 1,
                };
                let complete = projection
                    .propose(
                        TaskExecutionEvent::Completed { output },
                        projection.generation(),
                        TaskTick::new(projection.execution_deadline().get() - 1),
                    )
                    .unwrap();
                journal.commit(&complete, Some(b"done")).unwrap();
                projection.apply(&complete).unwrap();
            }
            assert_eq!(
                journal.begin_fence(scope(), projection.generation()),
                Ok(identity)
            );
            journal
                .confirm_stopped(scope(), projection.generation(), identity)
                .unwrap();
            journal
                .confirm_stopped(scope(), projection.generation(), identity)
                .unwrap();
            if generation < definition().recovery.max_attempts {
                let deadline = projection.execution_deadline();
                for tick in [deadline, deadline, TaskTick::new(deadline.get() + 1)] {
                    let record = projection.next(tick).unwrap().unwrap();
                    journal.commit(&record, None).unwrap();
                    projection.apply(&record).unwrap();
                }
            }
        }
        journal
            .retire_task(scope(), projection.next_sequence())
            .unwrap();
        for generation in 1..=definition().recovery.max_attempts {
            journal
                .confirm_stopped(scope(), TaskGeneration::new(generation), identity)
                .unwrap();
        }
        drop(journal);
        let mut reopened = open(&path);
        let connection = Connection::open(&path).unwrap();
        let before =
            write_snapshot::snapshot(&connection, &["runtime_generations", "task_retirements"]);
        let last = TaskGeneration::new(definition().recovery.max_attempts);
        assert_eq!(
            reopened.confirm_stopped(scope(), last, Some(Digest([8; 32]))),
            Err(TaskPortError::Conflict)
        );
        assert_eq!(
            write_snapshot::snapshot(&connection, &["runtime_generations", "task_retirements"]),
            before
        );
        reopened.confirm_stopped(scope(), last, identity).unwrap();
        assert_eq!(
            write_snapshot::snapshot(&connection, &["runtime_generations", "task_retirements"]),
            before
        );
        println!(
            "reserved={reserved} last_attempt=true idempotent=true retired_reopened=true wrong_identity_unchanged=true"
        );
    }
}
