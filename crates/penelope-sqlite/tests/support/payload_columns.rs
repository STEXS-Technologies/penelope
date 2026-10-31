//! Payload row borrows preserve type/bounds checks, ownership and quota accounting.
use super::*;
use penelope_ports::payloads::ProcessPayloadStore;
use rusqlite::types::Value;
#[test]
fn payload_row_validation_preserves_owned_reads_retry_errors_and_usage() {
    let bytes = vec![1; 16];
    let digest = ContentDigest::sha256(&bytes);
    for (name, stored, valid) in [
        ("original", Value::Blob(bytes.clone()), true),
        ("short", Value::Blob(vec![1; 15]), false),
        ("empty", Value::Blob(Vec::new()), false),
        ("different", Value::Blob(vec![2; 16]), false),
        ("oversized", Value::Blob(vec![1; 17]), false),
        ("text", Value::Text("x".repeat(16)), false),
        ("integer", Value::Integer(1), false),
        ("real", Value::Real(1.5), false),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("payloads.db");
        let scope = scope();
        let charge = u64::try_from(scope.canonical_bytes().len() + bytes.len() + 128).unwrap();
        let adapter = SqliteProcessStore::open(
            &path,
            SqliteLimits {
                max_blob_bytes: 16,
                max_scope_bytes: charge,
                ..SqliteLimits::default()
            },
            clock(),
            10,
            16,
        )
        .unwrap();
        block_on(adapter.put_payload(&scope, digest, &bytes)).unwrap();
        let held = block_on(adapter.read_payload(&scope, digest)).unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute("UPDATE process_payloads SET bytes=?1", [stored])
            .unwrap();
        let snapshot =
            write_snapshot::snapshot(&connection, &["process_payloads", "process_usage"]);
        let read = block_on(adapter.read_payload(&scope, digest));
        let retry = block_on(adapter.put_payload(&scope, digest, &bytes));
        println!(
            "case={name} valid={valid} read_ok={} retry_ok={}",
            read.is_ok(),
            retry.is_ok()
        );
        if valid {
            assert_eq!(read.unwrap(), bytes, "{name}");
            assert_eq!(retry.unwrap(), digest, "{name}");
        } else {
            assert_eq!(read, Err(PortError::Invariant), "{name}");
            assert_eq!(retry, Err(PortError::Invariant), "{name}");
        }
        assert_eq!(
            write_snapshot::snapshot(&connection, &["process_payloads", "process_usage"]),
            snapshot,
            "{name}"
        );
        connection
            .execute("UPDATE process_payloads SET bytes=?1", [&bytes])
            .unwrap();
        assert_eq!(
            block_on(adapter.read_payload(&scope, digest)).unwrap(),
            bytes
        );
        assert_eq!(
            block_on(adapter.put_payload(&scope, digest, &bytes)).unwrap(),
            digest
        );
        assert_eq!(held, bytes, "{name}");
        assert_eq!(
            block_on(adapter.put_payload(&scope, ContentDigest::sha256(b"x"), b"x")),
            Err(PortError::QuotaExceeded)
        );
        assert_eq!(
            block_on(adapter.put_payload(&scope, digest, b"x")),
            Err(PortError::Invariant)
        );
    }
}
#[test]
fn empty_and_maximum_payloads_survive_corruption_repair_and_reopen() {
    for size in [0_usize, 1048576] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("boundaries.db");
        let scope = scope();
        let bytes = vec![7; size];
        let digest = ContentDigest::sha256(&bytes);
        let adapter = store(&path, clock());
        block_on(adapter.put_payload(&scope, digest, &bytes)).unwrap();
        let held = block_on(adapter.read_payload(&scope, digest)).unwrap();
        assert_eq!(held, bytes);
        let connection = Connection::open(&path).unwrap();
        let used = write_snapshot::snapshot(&connection, &["process_usage"]);
        block_on(adapter.put_payload(&scope, digest, &bytes)).unwrap();
        assert_eq!(
            write_snapshot::snapshot(&connection, &["process_usage"]),
            used
        );
        connection
            .execute(
                "UPDATE process_payloads SET bytes=?1",
                [vec![8; size.max(1)]],
            )
            .unwrap();
        assert_eq!(
            block_on(adapter.read_payload(&scope, digest)),
            Err(PortError::Invariant)
        );
        assert_eq!(
            block_on(adapter.put_payload(&scope, digest, &bytes)),
            Err(PortError::Invariant)
        );
        assert_eq!(held, bytes);
        connection
            .execute("UPDATE process_payloads SET bytes=?1", [&bytes])
            .unwrap();
        drop(adapter);
        let reopened = store(&path, clock());
        assert_eq!(
            block_on(reopened.put_payload(&scope, digest, &bytes)).unwrap(),
            digest
        );
        assert_eq!(
            block_on(reopened.read_payload(&scope, digest)).unwrap(),
            bytes
        );
        assert_eq!(
            write_snapshot::snapshot(&connection, &["process_usage"]),
            used
        );
        println!("boundary_size={size} retry_repair_reopen=true");
    }
}
