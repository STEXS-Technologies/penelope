//! Repeated references retain complete validation and never cross replay calls.
use super::*;
#[test]
fn repeated_references_reject_changed_metadata_and_recheck_durable_bytes() {
    for damage in [
        "length-two",
        "length-four",
        "schema-two",
        "schema-four",
        "missing-input",
        "missing-state",
        "wrong-input",
        "wrong-state",
        "text-state",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("replay-memo.db");
        let mut journal = open(&path);
        let mut projection = committed_start(&mut journal);
        for (tick, bytes) in [
            (2, b"input".as_slice()),
            (3, b"state".as_slice()),
            (4, b"state".as_slice()),
            (5, b"input".as_slice()),
        ] {
            let record = projection
                .propose(
                    TaskExecutionEvent::CheckpointCommitted {
                        checkpoint: BlobRef {
                            digest: Digest::of(bytes),
                            bytes: 5,
                            schema: 1,
                        },
                    },
                    projection.generation(),
                    TaskTick::new(tick),
                )
                .unwrap();
            journal.commit(&record, Some(bytes)).unwrap();
            projection.apply(&record).unwrap();
        }
        let request = TaskStart {
            checkpoint: Some(b"input"),
            ..request(&projection)
        };
        let execution = Digest([9; 32]);
        assert_eq!(
            journal.reserve_start(request, execution),
            Ok(StartReservation::Reserved)
        );
        assert_eq!(
            journal.reserve_start(request, execution),
            Ok(StartReservation::AlreadyReserved)
        );
        journal.rebuild_head(scope()).unwrap();
        let next = projection
            .propose(
                TaskExecutionEvent::CheckpointCommitted {
                    checkpoint: BlobRef {
                        digest: Digest::of(b"input"),
                        bytes: 5,
                        schema: 1,
                    },
                },
                projection.generation(),
                TaskTick::new(6),
            )
            .unwrap();
        let connection = Connection::open(&path).unwrap();
        let tables = [
            "task_heads",
            "task_records",
            "task_blobs",
            "task_retirements",
            "runtime_generations",
        ];
        let healthy = write_snapshot::snapshot(&connection, &tables);
        if damage.starts_with("length") || damage.starts_with("schema") {
            let sequence = if damage.ends_with("two") { 2 } else { 4 };
            let encoded: Vec<u8> = connection
                .query_row(
                    "SELECT record FROM task_records WHERE sequence=?1",
                    [sequence],
                    |row| row.get(0),
                )
                .unwrap();
            let mut record: TaskRecord = serde_json::from_slice(&encoded).unwrap();
            let TaskExecutionEvent::CheckpointCommitted { ref mut checkpoint } = record.event
            else {
                panic!("checkpoint fixture");
            };
            if damage.starts_with("length") {
                checkpoint.bytes = 4;
            } else {
                checkpoint.schema = 2;
            }
            let updated_encoded = serde_json::to_vec(&record).unwrap();
            connection
                .execute(
                    "UPDATE task_records SET record=?1,checksum=?2 WHERE sequence=?3",
                    rusqlite::params![
                        &updated_encoded,
                        Digest::of(&updated_encoded).0.as_slice(),
                        sequence
                    ],
                )
                .unwrap();
        } else {
            let bytes = if damage.ends_with("input") {
                b"input".as_slice()
            } else {
                b"state".as_slice()
            };
            let digest = Digest::of(bytes);
            if damage.starts_with("missing") {
                connection
                    .execute(
                        "DELETE FROM task_blobs WHERE digest=?1",
                        [digest.0.as_slice()],
                    )
                    .unwrap();
            } else if damage.starts_with("text") {
                connection
                    .execute(
                        "UPDATE task_blobs SET bytes='state' WHERE digest=?1",
                        [digest.0.as_slice()],
                    )
                    .unwrap();
            } else {
                connection
                    .execute(
                        "UPDATE task_blobs SET bytes=x'77726f6e67' WHERE digest=?1",
                        [digest.0.as_slice()],
                    )
                    .unwrap();
            }
        }
        let damaged = write_snapshot::snapshot(&connection, &tables);
        assert_eq!(
            journal.commit(&next, Some(b"input")),
            Err(TaskPortError::Corrupt),
            "append {damage}"
        );
        assert_eq!(write_snapshot::snapshot(&connection, &tables), damaged);
        assert_eq!(
            journal.reserve_start(request, execution),
            Err(TaskPortError::Corrupt),
            "reserve {damage}"
        );
        assert_eq!(write_snapshot::snapshot(&connection, &tables), damaged);
        assert_eq!(
            journal.begin_fence(scope(), projection.generation()),
            Err(TaskPortError::Corrupt),
            "fence {damage}"
        );
        assert_eq!(write_snapshot::snapshot(&connection, &tables), damaged);
        assert_eq!(
            journal.rebuild_head(scope()),
            Err(TaskPortError::Corrupt),
            "rebuild {damage}"
        );
        assert_eq!(write_snapshot::snapshot(&connection, &tables), damaged);
        connection.execute_batch("BEGIN IMMEDIATE").unwrap();
        for (table, rows) in &healthy {
            connection
                .execute(&format!("DELETE FROM {table}"), [])
                .unwrap();
            for row in rows {
                let placeholders = vec!["?"; row.len()].join(",");
                connection
                    .execute(
                        &format!("INSERT INTO {table} VALUES ({placeholders})"),
                        rusqlite::params_from_iter(row),
                    )
                    .unwrap();
            }
        }
        connection.execute_batch("COMMIT").unwrap();
        assert_eq!(
            journal.reserve_start(request, execution),
            Ok(StartReservation::AlreadyReserved)
        );
        assert_eq!(journal.commit(&next, Some(b"input")), Ok(next));
        journal.rebuild_head(scope()).unwrap();
        drop(journal);
        let mut reopened = open(&path);
        assert_eq!(reopened.read(scope(), TaskSequence::new(6)), Ok(Some(next)));
        let mut buffer = [0; 5];
        assert_eq!(
            reopened.read_blob(scope(), projection.checkpoint().unwrap(), &mut buffer),
            Ok(5)
        );
        assert_eq!(&buffer, b"input");
        println!("damage={damage} four_rejections_unchanged=true repaired_reopened=true");
    }
}
