//! Generation metadata must validate before reservation or fencing is accepted.
use super::*;
use rusqlite::types::Value;
#[test]
fn generation_identity_columns_validate_in_every_durable_state() {
    let execution = Digest([9; 32]);
    let other = Digest([8; 32]);
    let mut unexpected = Vec::new();
    for state in [0, 1, 2] {
        for (name, value, identity) in [
            (
                "matching",
                Value::Blob(execution.0.to_vec()),
                Some(Some(execution)),
            ),
            (
                "different",
                Value::Blob(other.0.to_vec()),
                Some(Some(other)),
            ),
            ("null", Value::Null, Some(None)),
            ("empty", Value::Blob(Vec::new()), None),
            ("short", Value::Blob(vec![9; 31]), None),
            ("oversized", Value::Blob(vec![9; 33]), None),
            ("text", Value::Text("x".repeat(32)), None),
            ("integer", Value::Integer(1), None),
            ("real", Value::Real(1.5), None),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("authority-columns.db");
            let mut journal = open(&path);
            let projection = committed_start(&mut journal);
            assert_eq!(
                journal.reserve_start(request(&projection), execution),
                Ok(StartReservation::Reserved)
            );
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch("PRAGMA ignore_check_constraints=ON")
                .unwrap();
            connection
                .execute(
                    "UPDATE runtime_generations SET execution=?1,state=?2",
                    rusqlite::params![value, state],
                )
                .unwrap();
            connection
                .execute_batch("PRAGMA ignore_check_constraints=OFF")
                .unwrap();
            let tables = [
                "task_heads",
                "task_records",
                "task_blobs",
                "runtime_generations",
            ];
            let before = write_snapshot::snapshot(&connection, &tables);
            let expected = if identity.is_none() {
                Err(TaskPortError::Corrupt)
            } else if state == 0 && identity == Some(Some(execution)) {
                Ok(StartReservation::AlreadyReserved)
            } else {
                Err(TaskPortError::Conflict)
            };
            let result = journal.reserve_start(request(&projection), execution);
            println!("state={state} case={name} reserve={result:?}");
            if result != expected {
                unexpected.push(format!(
                    "state={state} case={name} expected={expected:?} actual={result:?}"
                ));
            }
            assert_eq!(write_snapshot::snapshot(&connection, &tables), before);
            let fenced = journal.begin_fence(scope(), projection.generation());
            if let Some(identity) = identity {
                assert_eq!(fenced, Ok(identity));
                let fenced_rows = write_snapshot::snapshot(&connection, &tables);
                let wrong = if identity == Some(other) {
                    Some(execution)
                } else {
                    Some(other)
                };
                assert_eq!(
                    journal.confirm_stopped(scope(), projection.generation(), wrong),
                    Err(TaskPortError::Conflict)
                );
                assert_eq!(write_snapshot::snapshot(&connection, &tables), fenced_rows);
                journal
                    .confirm_stopped(scope(), projection.generation(), identity)
                    .unwrap();
                journal
                    .confirm_stopped(scope(), projection.generation(), identity)
                    .unwrap();
            } else {
                assert_eq!(fenced, Err(TaskPortError::Corrupt));
                assert_eq!(write_snapshot::snapshot(&connection, &tables), before);
            }
            connection
                .execute(
                    "UPDATE runtime_generations SET execution=?1,state=0",
                    [execution.0.as_slice()],
                )
                .unwrap();
            assert_eq!(
                journal.reserve_start(request(&projection), execution),
                Ok(StartReservation::AlreadyReserved)
            );
            assert_eq!(
                journal.begin_fence(scope(), projection.generation()),
                Ok(Some(execution))
            );
            journal
                .confirm_stopped(scope(), projection.generation(), Some(execution))
                .unwrap();
            drop(journal);
            let mut reopened = open(&path);
            assert_eq!(
                reopened.reserve_start(request(&projection), execution),
                Err(TaskPortError::Conflict)
            );
            assert_eq!(
                reopened.begin_fence(scope(), projection.generation()),
                Ok(Some(execution))
            );
            reopened
                .confirm_stopped(scope(), projection.generation(), Some(execution))
                .unwrap();
        }
    }
    assert!(unexpected.is_empty(), "{}", unexpected.join("\n"));
}

#[test]
fn authorization_head_query_rebinds_scope_and_observes_repairs_and_ddl() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("head-rebinding.db");
    let mut journal = open(&path);
    let projection = committed_start(&mut journal);
    let mut other_scope = scope();
    other_scope.task = TaskId::from_digest(Digest([77; 32]));
    let mut other_intent = intent();
    other_intent.scope = other_scope;
    journal.commit(&other_intent, Some(b"input")).unwrap();
    let mut other_projection = TaskProjection::new(other_scope, definition()).unwrap();
    other_projection.apply(&other_intent).unwrap();
    let start = other_projection.next(TaskTick::new(1)).unwrap().unwrap();
    journal.commit(&start, None).unwrap();
    other_projection.apply(&start).unwrap();
    let execution = Digest([9; 32]);
    let mut other_request = request(&other_projection);
    other_request.scope = other_scope;
    assert_eq!(
        journal.reserve_start(request(&projection), execution),
        Ok(StartReservation::Reserved)
    );
    assert_eq!(
        journal.reserve_start(other_request, execution),
        Ok(StartReservation::Reserved)
    );
    let connection = Connection::open(&path).unwrap();
    for ddl in [
        "CREATE INDEX authority_probe ON task_blobs(digest)",
        "DROP INDEX authority_probe",
    ] {
        connection.execute_batch(ddl).unwrap();
        for requested in [request(&projection), other_request, request(&projection)] {
            assert_eq!(
                journal.reserve_start(requested, execution),
                Ok(StartReservation::AlreadyReserved)
            );
        }
        let key = [
            scope().tenant.digest().0,
            scope().task.digest().0,
            scope().definition.digest().0,
            scope().input.digest().0,
        ]
        .concat();
        connection
            .execute(
                "UPDATE task_heads SET bytes_used=bytes_used+1 WHERE scope=?1",
                [&key],
            )
            .unwrap();
        assert_eq!(
            journal.reserve_start(request(&projection), execution),
            Err(TaskPortError::Corrupt)
        );
        assert_eq!(
            journal.reserve_start(other_request, execution),
            Ok(StartReservation::AlreadyReserved)
        );
        connection
            .execute(
                "UPDATE task_heads SET bytes_used=bytes_used-1 WHERE scope=?1",
                [&key],
            )
            .unwrap();
        assert_eq!(
            journal.reserve_start(request(&projection), execution),
            Ok(StartReservation::AlreadyReserved)
        );
    }
}
