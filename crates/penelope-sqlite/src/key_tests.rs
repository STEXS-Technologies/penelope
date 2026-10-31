//! Persistent key equivalence and cross-representation SQLite binding.
#![allow(clippy::unwrap_used)]
use super::*;

fn scope(seed: u32) -> TaskScope {
    TaskScope {
        tenant: TaskTenantId::from_digest(Digest::of(&seed.to_le_bytes())),
        task: TaskId::from_digest(Digest::of(&seed.to_be_bytes())),
        definition: TaskDefinitionFingerprint::from_digest(Digest::of(
            &seed.wrapping_add(1).to_le_bytes(),
        )),
        input: TaskInputDigest::from_digest(Digest::of(&seed.wrapping_add(2).to_be_bytes())),
    }
}
fn legacy(scope: TaskScope) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(128);
    for digest in [
        scope.tenant.digest(),
        scope.task.digest(),
        scope.definition.digest(),
        scope.input.digest(),
    ] {
        bytes.extend_from_slice(&digest.0);
    }
    bytes
}
#[test]
fn fixed_scope_keys_preserve_all_legacy_bytes() {
    for seed in 0..10_000 {
        let scope = scope(seed);
        assert_eq!(key(scope).as_slice(), legacy(scope));
        for changed in [
            TaskScope {
                tenant: TaskTenantId::from_digest(Digest([17; 32])),
                ..scope
            },
            TaskScope {
                task: TaskId::from_digest(Digest([18; 32])),
                ..scope
            },
            TaskScope {
                definition: TaskDefinitionFingerprint::from_digest(Digest([19; 32])),
                ..scope
            },
            TaskScope {
                input: TaskInputDigest::from_digest(Digest([20; 32])),
                ..scope
            },
        ] {
            assert_ne!(key(scope), key(changed));
        }
    }
}
#[test]
fn array_and_legacy_keys_bind_as_identical_sqlite_blobs() {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch("CREATE TABLE scope_keys(scope BLOB PRIMARY KEY,value INTEGER NOT NULL)")
        .unwrap();
    for seed in 0..256 {
        let scope = scope(seed);
        let fixed = key(scope);
        let legacy = legacy(scope);
        let (kind, length, bytes): (String, u32, Vec<u8>) = connection
            .query_row("SELECT typeof(?1),length(?1),?1", [fixed], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap();
        assert_eq!(kind, "blob");
        assert_eq!(length, 128);
        assert_eq!(bytes, legacy);
        connection
            .execute(
                "INSERT INTO scope_keys VALUES (?1,?2)",
                params![legacy, seed],
            )
            .unwrap();
        let value: u32 = connection
            .query_row(
                "SELECT value FROM scope_keys WHERE scope=?1",
                [fixed],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(value, seed);
        connection
            .execute(
                "UPDATE scope_keys SET value=?2 WHERE scope=?1",
                params![fixed, seed.wrapping_add(1)],
            )
            .unwrap();
        let updated: u32 = connection
            .query_row(
                "SELECT value FROM scope_keys WHERE scope=?1",
                [legacy],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(updated, seed.wrapping_add(1));
    }
}
