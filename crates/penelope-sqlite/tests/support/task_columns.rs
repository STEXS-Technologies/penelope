//! Borrowed task record columns preserve bounded decoding and owned results.
use super::*;
use rusqlite::types::Value;
#[test]
fn task_record_columns_preserve_validation_and_owned_results() {
    let record = intent();
    let original = serde_json::to_vec(&record).unwrap();
    let text = String::from_utf8(original.clone()).unwrap();
    let scope_text = serde_json::to_string(&record.scope).unwrap();
    let pretty_scope = serde_json::to_string_pretty(&record.scope).unwrap();
    let mut sequence_scope = serde_json::to_value(record).unwrap();
    sequence_scope["scope"] = serde_json::json!([
        record.scope.tenant,
        record.scope.task,
        record.scope.definition,
        record.scope.input
    ]);
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
        (
            "pretty-scope",
            text.replacen(&scope_text, &pretty_scope, 1).into_bytes(),
            true,
        ),
        (
            "reordered-fields",
            serde_json::to_vec(&serde_json::to_value(record).unwrap()).unwrap(),
            true,
        ),
        (
            "scope-sequence",
            serde_json::to_vec(&sequence_scope).unwrap(),
            true,
        ),
        (
            "escaped-fields",
            text.replace("\"scope\"", r#""\u0073cope""#)
                .replace("\"tenant\"", r#""\u0074enant""#)
                .into_bytes(),
            true,
        ),
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
        let rebuilt = journal.rebuild_head(scope());
        if valid {
            assert_eq!(rebuilt, Ok(()), "rebuild case={name}");
            let Value::Blob(stored_bytes) = &bytes else {
                panic!("valid fixture must be a BLOB")
            };
            let (next, digest, used): (u32, Vec<u8>, u64) = connection
                .query_row(
                    "SELECT next_sequence,last_digest,bytes_used FROM task_heads",
                    [],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            u64::try_from(row.get::<_, i64>(2)?).unwrap(),
                        ))
                    },
                )
                .unwrap();
            assert_eq!(next, 1);
            assert_eq!(digest, Digest::of(stored_bytes).0);
            assert_eq!(used, u64::try_from(stored_bytes.len()).unwrap() + 5);
        } else {
            assert_eq!(rebuilt, Err(TaskPortError::Corrupt), "rebuild case={name}");
            assert_eq!(write_snapshot::snapshot(&connection, &tables), before);
        }
        connection
            .execute(
                "UPDATE task_records SET record=?1,checksum=?2",
                rusqlite::params![original, Digest::of(&original).0.as_slice()],
            )
            .unwrap();
        journal.rebuild_head(scope()).unwrap();
        assert_eq!(journal.read(scope(), TaskSequence::ZERO).unwrap(), held);
        assert_eq!(held, Some(record));
        let mut projection = TaskProjection::new(scope(), definition()).unwrap();
        projection.apply(&record).unwrap();
        let start = projection.next(TaskTick::new(1)).unwrap().unwrap();
        assert_eq!(journal.commit(&start, None).unwrap(), start);
        drop(journal);
        assert_eq!(open(&path).read(scope(), TaskSequence::ZERO).unwrap(), held);
    }
}
