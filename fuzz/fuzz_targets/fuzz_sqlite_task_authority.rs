#![no_main]
// A real WAL/FULL database and an independent connection exercise durable
// authority state, not a mock. Each input owns and removes its bounded database.
#[path = "../../crates/penelope-executor/tests/support/tasks.rs"]
mod support;

use libfuzzer_sys::fuzz_target;
use penelope_core::task::{runtime::*, *};
use penelope_sqlite::{SqliteLimits, SqliteTaskJournal};

fuzz_target!(|data: &[u8]| {
    if data.len() > 256 {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("authority.db");
    let definition = support::definition(RecoveryMode::RestartFromBeginning, EffectContract::Pure);
    let limits = SqliteLimits {
        max_blob_bytes: 64,
        max_record_bytes: 4096,
        max_history_records: 512,
        max_scope_bytes: 65536,
        max_scopes: 2,
        max_database_bytes: 1024 * 1024,
        ..SqliteLimits::default()
    };
    let input = &data[..data.len().min(64)];
    let scope = support::scope(definition, input);
    let reference = BlobRef {
        digest: Digest::of(input),
        bytes: input.len() as u32,
        schema: 1,
    };
    let mut journal = SqliteTaskJournal::open(&path, definition, limits).unwrap();
    let mut projection = TaskProjection::new(scope, definition).unwrap();
    let intent = projection
        .propose(
            TaskExecutionEvent::IntentCommitted {
                definition,
                input: reference,
            },
            TaskGeneration::ZERO,
            TaskTick::ZERO,
        )
        .unwrap();
    // Failed blob verification must not consume sequence zero.
    let mut wrong = input.to_vec();
    wrong.push(0);
    assert!(journal.commit(&intent, Some(&wrong)).is_err());
    assert_eq!(journal.read(scope, TaskSequence::ZERO).unwrap(), None);
    assert_eq!(journal.commit(&intent, Some(input)).unwrap(), intent);
    projection.apply(&intent).unwrap();
    let start = projection.next(TaskTick::new(1)).unwrap().unwrap();
    journal.commit(&start, None).unwrap();
    projection.apply(&start).unwrap();
    let request = TaskStart {
        scope,
        generation: projection.generation(),
        definition,
        execution_deadline: projection.execution_deadline(),
        input_schema: 1,
        input,
        checkpoint: None,
    };
    let execution = Digest([7; 32]);
    let other = Digest([8; 32]);
    // Model: optional reserved identity plus irreversible fence/stopped bits.
    let mut reserved = false;
    let mut fenced = false;
    let mut stopped = false;
    for operation in data.iter().copied().take(16).chain([0, 2, 3, 0]) {
        // A newly opened connection must observe the same durable authority.
        let mut contender = SqliteTaskJournal::open(&path, definition, limits).unwrap();
        match operation % 8 {
            0 => {
                let result = contender.reserve_start(request, execution);
                let expected = if fenced {
                    Err(TaskPortError::Conflict)
                } else if reserved {
                    Ok(StartReservation::AlreadyReserved)
                } else {
                    Ok(StartReservation::Reserved)
                };
                assert_eq!(result, expected);
                if result.is_ok() {
                    reserved = true;
                }
            }
            1 => {
                // A competing identity never steals a previously reserved slot.
                if reserved || fenced {
                    assert_eq!(
                        contender.reserve_start(request, other),
                        Err(TaskPortError::Conflict)
                    );
                }
            }
            2 => {
                assert_eq!(
                    contender.begin_fence(scope, request.generation).unwrap(),
                    reserved.then_some(execution)
                );
                fenced = true;
            }
            3 => {
                let result = contender.confirm_stopped(
                    scope,
                    request.generation,
                    reserved.then_some(execution),
                );
                assert_eq!(
                    result,
                    if fenced {
                        Ok(())
                    } else {
                        Err(TaskPortError::Conflict)
                    }
                );
                if result.is_ok() {
                    stopped = true;
                }
            }
            4 => {
                assert_eq!(
                    contender.confirm_stopped(scope, request.generation, Some(other)),
                    Err(TaskPortError::Conflict)
                );
            }
            5 => {
                let substituted = TaskStart {
                    input_schema: 2,
                    ..request
                };
                assert!(contender.reserve_start(substituted, execution).is_err());
            }
            6 => {
                assert_eq!(
                    contender.commit(&intent, Some(input)),
                    Err(TaskPortError::Conflict)
                );
                assert_eq!(contender.commit(&start, None), Err(TaskPortError::Conflict));
            }
            _ => {
                drop(journal);
                journal = SqliteTaskJournal::open(&path, definition, limits).unwrap();
            }
        }
        // Journal replay and exact immutable bytes are independent of lease
        // traffic, duplicate commits and connection reconstruction.
        let records: Vec<_> = (0..2)
            .map(|sequence| {
                journal
                    .read(scope, TaskSequence::new(sequence))
                    .unwrap()
                    .unwrap()
            })
            .collect();
        assert_eq!(replay(scope, definition, records).unwrap(), projection);
        assert_eq!(journal.read(scope, TaskSequence::new(2)).unwrap(), None);
        let mut bytes = [0; 64];
        let length = journal.read_blob(scope, reference, &mut bytes).unwrap();
        assert_eq!(&bytes[..length], input);
        if stopped {
            assert_eq!(
                journal.reserve_start(request, execution),
                Err(TaskPortError::Conflict)
            );
        }
    }
});
