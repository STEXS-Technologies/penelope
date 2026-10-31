//! Borrowed task record columns preserve bounded decoding and owned results.
use super::*;
use rusqlite::types::Value;
#[test]
fn task_record_columns_preserve_validation_and_owned_results() {
    let record = intent();
    let original = serde_json::to_vec(&record).unwrap();
    let mut padded = original.clone();
    assert!(padded.len() < 4096);
    padded.resize(4096, b' ');
    let mut oversized = padded.clone();
    oversized.push(b' ');
    let mut wrong_scope = record;
    wrong_scope.scope.task = TaskId::from_digest(Digest([99; 32]));
    let mut wrong_sequence = record;
    wrong_sequence.sequence = TaskSequence::new(1);
    let mut wrong_version = record;
    wrong_version.version = 2;
    let mut cases = Vec::new();
    for (name, bytes, valid) in [
        ("original", original.clone(), true),
        ("at-limit", padded, true),
        ("over-limit", oversized, false),
        ("empty", Vec::new(), false),
        ("malformed", b"{".to_vec(), false),
        ("null", b"null".to_vec(), false),
        ("trailing", [original.as_slice(), b"{}"].concat(), false),
        ("utf8", vec![255], false),
        ("scope", serde_json::to_vec(&wrong_scope).unwrap(), false),
        (
            "sequence",
            serde_json::to_vec(&wrong_sequence).unwrap(),
            false,
        ),
        (
            "version",
            serde_json::to_vec(&wrong_version).unwrap(),
            false,
        ),
    ] {
        let checksum = Value::Blob(Digest::of(&bytes).0.to_vec());
        cases.push((name, Value::Blob(bytes), checksum, valid));
    }
    let original_checksum = Value::Blob(Digest::of(&original).0.to_vec());
    for (name, value) in [
        (
            "record-text",
            Value::Text(String::from_utf8(original.clone()).unwrap()),
        ),
        ("record-integer", Value::Integer(1)),
        ("record-real", Value::Real(1.5)),
    ] {
        cases.push((name, value, original_checksum.clone(), false));
    }
    for (name, value) in [
        ("checksum-text", Value::Text("x".repeat(32))),
        ("checksum-integer", Value::Integer(1)),
        ("checksum-real", Value::Real(1.5)),
        ("checksum-empty", Value::Blob(Vec::new())),
        ("checksum-short", Value::Blob(vec![0; 31])),
        ("checksum-wrong", Value::Blob(vec![0; 32])),
        ("checksum-long", Value::Blob(vec![0; 33])),
    ] {
        cases.push((name, Value::Blob(original.clone()), value, false));
    }
    for (name, bytes, checksum, valid) in cases {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("task-columns.db");
        let mut journal = SqliteTaskJournal::open(
            &path,
            definition(),
            SqliteLimits {
                max_record_bytes: 4096,
                ..SqliteLimits::default()
            },
        )
        .unwrap();
        journal.commit(&record, Some(b"input")).unwrap();
        let held = journal.read(scope(), TaskSequence::ZERO).unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("PRAGMA ignore_check_constraints=ON")
            .unwrap();
        connection
            .execute(
                "UPDATE task_records SET record=?1,checksum=?2",
                rusqlite::params![bytes, checksum],
            )
            .unwrap();
        connection
            .execute_batch("PRAGMA ignore_check_constraints=OFF")
            .unwrap();
        let tables = ["task_records", "task_heads", "task_blobs"];
        let before = write_snapshot::snapshot(&connection, &tables);
        let result = journal.read(scope(), TaskSequence::ZERO);
        if valid {
            assert_eq!(result, Ok(Some(record)), "case={name}");
        } else {
            assert_eq!(result, Err(TaskPortError::Corrupt), "case={name}");
        }
        assert_eq!(write_snapshot::snapshot(&connection, &tables), before);
        connection
            .execute(
                "UPDATE task_records SET record=?1,checksum=?2",
                rusqlite::params![original, Digest::of(&original).0.as_slice()],
            )
            .unwrap();
        assert_eq!(journal.read(scope(), TaskSequence::ZERO).unwrap(), held);
        assert_eq!(held, Some(record));
        drop(journal);
        assert_eq!(open(&path).read(scope(), TaskSequence::ZERO).unwrap(), held);
    }
}
