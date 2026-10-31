//! Borrowed row bytes retain the original type, size, checksum and record checks.
use super::*;
use rusqlite::types::Value;
#[test]
fn replay_column_validation_and_owned_results_survive_warm_statement_reuse() {
    let expected = commit(false);
    let original = serde_json::to_vec(&expected.outcomes[0]).unwrap();
    let checksum = ContentDigest::sha256(&original).0.to_vec();
    assert!(original.len() < 4096);
    let mut padded = original.clone();
    padded.resize(4096, b' ');
    let mut oversized = padded.clone();
    oversized.push(b' ');
    let mut wrong_scope = expected.outcomes[0].clone();
    wrong_scope.process_id = id("prc_wrong");
    let mut wrong_sequence = expected.outcomes[0].clone();
    wrong_sequence.sequence = 1;
    let encoded_cases = [
        ("original", original.clone(), true),
        ("at-limit", padded, true),
        ("over-limit", oversized, false),
        ("empty", Vec::new(), false),
        ("malformed-json", b"{".to_vec(), false),
        ("json-null", b"null".to_vec(), false),
        (
            "trailing-json",
            [original.as_slice(), b"{}"].concat(),
            false,
        ),
        ("invalid-utf8", vec![255], false),
        (
            "wrong-scope",
            serde_json::to_vec(&wrong_scope).unwrap(),
            false,
        ),
        (
            "wrong-sequence",
            serde_json::to_vec(&wrong_sequence).unwrap(),
            false,
        ),
    ];
    let mut cases = Vec::new();
    for (name, bytes, valid) in encoded_cases {
        let digest = ContentDigest::sha256(&bytes).0.to_vec();
        cases.push((name, Value::Blob(bytes), Value::Blob(digest), valid));
    }
    for (name, record) in [
        (
            "text-record",
            Value::Text(String::from_utf8(original.clone()).unwrap()),
        ),
        ("integer-record", Value::Integer(1)),
        ("real-record", Value::Real(1.5)),
    ] {
        cases.push((name, record, Value::Blob(checksum.clone()), false));
    }
    for (name, digest) in [
        ("text-checksum", Value::Text("x".repeat(32))),
        ("integer-checksum", Value::Integer(1)),
        ("real-checksum", Value::Real(1.5)),
        ("empty-checksum", Value::Blob(Vec::new())),
        ("short-checksum", Value::Blob(vec![0; 31])),
        ("wrong-checksum", Value::Blob(vec![0; 32])),
        ("long-checksum", Value::Blob(vec![0; 33])),
    ] {
        cases.push((name, Value::Blob(original.clone()), digest, false));
    }
    for (name, record, digest, valid) in cases {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("columns.db");
        let adapter = SqliteProcessStore::open(
            &path,
            SqliteLimits {
                max_record_bytes: 4096,
                ..SqliteLimits::default()
            },
            clock(),
            10,
            16,
        )
        .unwrap();
        block_on(ProcessStore::commit(&adapter, &expected)).unwrap();
        let held = block_on(adapter.read_outcomes(&replay())).unwrap();
        assert_eq!(held.outcomes, expected.outcomes);
        let connection = Connection::open(&path).unwrap();
        connection
            .execute(
                "UPDATE process_outcomes SET record=?1,checksum=?2",
                rusqlite::params![record, digest],
            )
            .unwrap();
        let before = write_snapshot::snapshot(&connection, &["process_outcomes", "process_heads"]);
        let result = block_on(adapter.read_outcomes(&replay()));
        println!("case={name} valid={valid} accepted={}", result.is_ok());
        if valid {
            let page = result.unwrap();
            page.validate().unwrap();
            assert_eq!(page.outcomes, expected.outcomes, "{name}");
        } else {
            assert_eq!(result, Err(PortError::Invariant), "{name}");
        }
        assert_eq!(
            write_snapshot::snapshot(&connection, &["process_outcomes", "process_heads"]),
            before,
            "{name}"
        );
        connection
            .execute(
                "UPDATE process_outcomes SET record=?1,checksum=?2",
                rusqlite::params![original, checksum],
            )
            .unwrap();
        let recovered = block_on(adapter.read_outcomes(&replay())).unwrap();
        assert_eq!(recovered.outcomes, expected.outcomes, "{name}");
        assert_eq!(held.outcomes, expected.outcomes, "{name}");
    }
}
