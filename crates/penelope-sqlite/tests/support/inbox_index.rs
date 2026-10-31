//! Version-six pending-inbox index migration preserves rows and recovery behavior.
use super::*;
use penelope_ports::inbox::PendingInbox;
const TABLES: &[&str] = &[
    "process_usage",
    "process_heads",
    "process_outcomes",
    "process_inbox",
    "process_sources",
    "process_commits",
    "process_actions",
    "process_timers",
    "process_payloads",
    "lease_counter",
];
fn version(connection: &Connection) -> u32 {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap()
}
fn index_exists(connection: &Connection) -> bool {
    connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='index' AND name='process_inbox_pending' AND tbl_name='process_inbox')", [], |row| row.get(0)).unwrap()
}
fn downgrade(connection: &Connection) {
    connection
        .execute_batch("DROP INDEX process_inbox_pending; PRAGMA user_version=5;")
        .unwrap();
}
#[test]
fn version_five_upgrade_preserves_pending_inputs_receipts_and_live_leases() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("inbox-upgrade.db");
    let time = clock();
    let original = store(&path, time.clone());
    let first = commit(true);
    block_on(ProcessStore::commit(&original, &first)).unwrap();
    block_on(original.schedule(&TimerSchedule {
        action: first.actions[0].clone(),
        due_at: LogicalTime(100),
    }))
    .unwrap();
    let outbox = block_on(original.claim(&claim())).unwrap().remove(0);
    let timer = block_on(original.claim_due(&timer_claim(100)))
        .unwrap()
        .remove(0);
    let mut pending = first.input.clone().unwrap();
    pending.input_id = id("inp_a");
    let mut later = pending.clone();
    later.input_id = id("inp_z");
    block_on(original.accept(&later)).unwrap();
    block_on(original.accept(&pending)).unwrap();
    drop(original);
    let connection = Connection::open(&path).unwrap();
    downgrade(&connection);
    let before = write_snapshot::snapshot(&connection, TABLES);
    let upgraded = store(&path, time.clone());
    assert_eq!(version(&connection), 6);
    assert!(index_exists(&connection));
    assert_eq!(write_snapshot::snapshot(&connection, TABLES), before);
    let scope = scope();
    let discover = || {
        block_on(upgraded.pending_inputs(
            &scope.tenant_id,
            &scope.process_id,
            NonZeroU16::new(64).unwrap(),
        ))
        .unwrap()
    };
    assert_eq!(discover(), vec![pending.clone(), later.clone()]);
    assert_eq!(discover(), vec![pending.clone(), later.clone()]);
    assert!(block_on(upgraded.claim(&claim())).unwrap().is_empty());
    assert!(
        block_on(upgraded.claim_due(&timer_claim(100)))
            .unwrap()
            .is_empty()
    );
    block_on(OutboxStore::acknowledge(
        &upgraded,
        &outbox,
        LogicalTime(100),
    ))
    .unwrap();
    block_on(TimerClaimStore::acknowledge(
        &upgraded,
        &timer,
        LogicalTime(100),
    ))
    .unwrap();
    let mut outcome = first.outcomes[0].clone();
    outcome.sequence = 1;
    outcome.outcome_id = id("out_pending");
    outcome.causation_id = CausationId::Input(pending.input_id.clone());
    outcome.kind = ProcessOutcomeKind::ActionPlanned;
    let next = AtomicProcessCommit::new(1, Some(pending), vec![outcome], Vec::new()).unwrap();
    block_on(ProcessStore::commit(&upgraded, &next))
        .unwrap()
        .validate_for(&next)
        .unwrap();
    assert_eq!(discover(), vec![later.clone()]);
    drop(upgraded);
    let reopened = store(&path, time);
    for batch in [&first, &next] {
        let receipt = block_on(ProcessStore::commit(&reopened, batch)).unwrap();
        receipt.validate_for(batch).unwrap();
        assert!(receipt.duplicate_input);
    }
    assert_eq!(
        block_on(reopened.pending_inputs(
            &scope.tenant_id,
            &scope.process_id,
            NonZeroU16::new(64).unwrap()
        ))
        .unwrap(),
        vec![later]
    );
}
#[test]
fn inbox_index_collision_rolls_back_version_and_rows_until_repaired() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("collision.db");
    let original = store(&path, clock());
    let batch = commit(false);
    block_on(original.accept(batch.input.as_ref().unwrap())).unwrap();
    drop(original);
    let connection = Connection::open(&path).unwrap();
    downgrade(&connection);
    connection
        .execute_batch("CREATE INDEX process_inbox_pending ON process_actions(scope)")
        .unwrap();
    let before = write_snapshot::snapshot(&connection, TABLES);
    assert!(SqliteProcessStore::open(&path, SqliteLimits::default(), clock(), 10, 16).is_err());
    assert_eq!(version(&connection), 5);
    assert!(!index_exists(&connection));
    assert_eq!(write_snapshot::snapshot(&connection, TABLES), before);
    connection
        .execute_batch("DROP INDEX process_inbox_pending")
        .unwrap();
    let repaired = store(&path, clock());
    assert_eq!(version(&connection), 6);
    assert!(index_exists(&connection));
    assert_eq!(write_snapshot::snapshot(&connection, TABLES), before);
    let scope = scope();
    assert_eq!(
        block_on(repaired.pending_inputs(
            &scope.tenant_id,
            &scope.process_id,
            NonZeroU16::new(1).unwrap()
        ))
        .unwrap(),
        vec![batch.input.unwrap()]
    );
}
#[test]
fn inbox_index_page_limit_failure_preserves_version_and_pending_rows() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("full.db");
    let original = store(&path, clock());
    let input = commit(false).input.unwrap();
    block_on(original.accept(&input)).unwrap();
    drop(original);
    let connection = Connection::open(&path).unwrap();
    downgrade(&connection);
    connection.execute_batch("CREATE TABLE padding(bytes BLOB); INSERT INTO padding VALUES(zeroblob(131072)); VACUUM;").unwrap();
    let pages: u32 = connection
        .query_row("PRAGMA page_count", [], |row| row.get(0))
        .unwrap();
    let size: u32 = connection
        .query_row("PRAGMA page_size", [], |row| row.get(0))
        .unwrap();
    let free: u32 = connection
        .query_row("PRAGMA freelist_count", [], |row| row.get(0))
        .unwrap();
    assert_eq!(free, 0);
    let before = write_snapshot::snapshot(&connection, TABLES);
    let limits = SqliteLimits {
        max_database_bytes: u64::from(pages) * u64::from(size),
        ..SqliteLimits::default()
    };
    assert!(matches!(
        SqliteProcessStore::open(&path, limits, clock(), 10, 16),
        Err(PortError::QuotaExceeded)
    ));
    assert_eq!(version(&connection), 5);
    assert!(!index_exists(&connection));
    assert_eq!(write_snapshot::snapshot(&connection, TABLES), before);
    let repaired = store(&path, clock());
    assert_eq!(version(&connection), 6);
    assert!(index_exists(&connection));
    assert_eq!(write_snapshot::snapshot(&connection, TABLES), before);
    let scope = scope();
    assert_eq!(
        block_on(repaired.pending_inputs(
            &scope.tenant_id,
            &scope.process_id,
            NonZeroU16::new(1).unwrap()
        ))
        .unwrap(),
        vec![input]
    );
}

#[test]
fn next_unsupported_schema_rejects_healthy_database_without_row_changes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("future.db");
    let original = store(&path, clock());
    block_on(ProcessStore::commit(&original, &commit(false))).unwrap();
    drop(original);
    let connection = Connection::open(&path).unwrap();
    connection.pragma_update(None, "user_version", 7).unwrap();
    let before = write_snapshot::snapshot(&connection, TABLES);
    assert!(matches!(
        SqliteProcessStore::open(&path, SqliteLimits::default(), clock(), 10, 16),
        Err(PortError::Invariant)
    ));
    assert_eq!(version(&connection), 7);
    assert_eq!(write_snapshot::snapshot(&connection, TABLES), before);
}
