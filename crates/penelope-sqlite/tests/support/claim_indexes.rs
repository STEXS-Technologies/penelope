//! Additive index migration must preserve authoritative rows and live leases.
use super::write_snapshot::snapshot;
use super::*;
const INDEXES: &str = "DROP INDEX process_actions_pending; DROP INDEX process_timers_due; DROP INDEX process_inbox_pending; PRAGMA user_version=4;";
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
fn index_names(connection: &Connection) -> Vec<String> {
    connection.prepare("SELECT name FROM sqlite_master WHERE type='index' AND name IN ('process_actions_pending','process_timers_due') ORDER BY name").unwrap()
        .query_map([],|row|row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap()
}
fn version(connection: &Connection) -> u32 {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap()
}
#[test]
fn version_four_upgrade_preserves_live_outbox_and_timer_leases() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("upgrade.db");
    let clock = clock();
    let original = store(&path, clock.clone());
    let batch = commit(true);
    block_on(ProcessStore::commit(&original, &batch)).unwrap();
    block_on(original.schedule(&TimerSchedule {
        action: batch.actions[0].clone(),
        due_at: LogicalTime(100),
    }))
    .unwrap();
    let outbox = block_on(original.claim(&claim())).unwrap().remove(0);
    let timer = block_on(original.claim_due(&timer_claim(100)))
        .unwrap()
        .remove(0);
    drop(original);
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch(INDEXES).unwrap();
    let before = snapshot(&connection, TABLES);
    let upgraded = store(&path, clock);
    assert_eq!(version(&connection), 6);
    assert_eq!(
        index_names(&connection),
        ["process_actions_pending", "process_timers_due"]
    );
    assert_eq!(snapshot(&connection, TABLES), before);
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
    assert!(
        block_on(upgraded.claim_due(&timer_claim(111)))
            .unwrap()
            .is_empty()
    );
}
#[test]
fn second_index_creation_failure_rolls_back_version_first_index_and_rows() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("failure.db");
    let original = store(&path, clock());
    block_on(ProcessStore::commit(&original, &commit(false))).unwrap();
    drop(original);
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch(INDEXES).unwrap();
    connection
        .execute_batch("CREATE INDEX process_timers_due ON process_actions(scope)")
        .unwrap();
    let before = snapshot(&connection, TABLES);
    assert!(SqliteProcessStore::open(&path, SqliteLimits::default(), clock(), 10, 16).is_err());
    assert_eq!(version(&connection), 4);
    assert_eq!(index_names(&connection), ["process_timers_due"]);
    assert_eq!(snapshot(&connection, TABLES), before);
    connection
        .execute_batch("DROP INDEX process_timers_due")
        .unwrap();
    let repaired = store(&path, clock());
    assert_eq!(version(&connection), 6);
    assert_eq!(
        index_names(&connection),
        ["process_actions_pending", "process_timers_due"]
    );
    assert_eq!(snapshot(&connection, TABLES), before);
    assert_eq!(block_on(repaired.claim(&claim())).unwrap().len(), 1);
}
#[test]
fn index_upgrade_respects_database_page_limit_and_recovers_after_limit_increase() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("full.db");
    let original = store(&path, clock());
    block_on(ProcessStore::commit(&original, &commit(false))).unwrap();
    drop(original);
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch(INDEXES).unwrap();
    // Reserve enough existing pages for the adapter's minimum database bound,
    // then remove free pages so building either index requires new capacity.
    connection.execute_batch("CREATE TABLE padding(bytes BLOB); INSERT INTO padding VALUES(zeroblob(131072)); VACUUM;").unwrap();
    let pages: u32 = connection
        .query_row("PRAGMA page_count", [], |row| row.get(0))
        .unwrap();
    let page_size: u32 = connection
        .query_row("PRAGMA page_size", [], |row| row.get(0))
        .unwrap();
    let free: u32 = connection
        .query_row("PRAGMA freelist_count", [], |row| row.get(0))
        .unwrap();
    assert_eq!(free, 0);
    let before = snapshot(&connection, TABLES);
    let limits = SqliteLimits {
        max_database_bytes: u64::from(pages) * u64::from(page_size),
        ..SqliteLimits::default()
    };
    assert!(matches!(
        SqliteProcessStore::open(&path, limits, clock(), 10, 16),
        Err(PortError::QuotaExceeded)
    ));
    assert_eq!(version(&connection), 4);
    assert!(index_names(&connection).is_empty());
    assert_eq!(snapshot(&connection, TABLES), before);
    let repaired = store(&path, clock());
    assert_eq!(version(&connection), 6);
    assert_eq!(snapshot(&connection, TABLES), before);
    assert_eq!(block_on(repaired.claim(&claim())).unwrap().len(), 1);
}

#[test]
fn indexed_timer_claims_preserve_due_order_ties_limits_and_expiry_boundaries() {
    let directory = tempfile::tempdir().unwrap();
    let adapter = store(&directory.path().join("timer-order.db"), clock());
    let mut batch = commit(true);
    let template = batch.actions[0].clone();
    batch.actions.clear();
    for name in ["z", "a", "b", "c", "d", "e", "f"] {
        let mut action = template.clone();
        action.action_id = id(&format!("act_{name}"));
        action.step_id = id(&format!("stp_{name}"));
        batch.actions.push(action);
    }
    block_on(ProcessStore::commit(&adapter, &batch)).unwrap();
    for (action, due) in batch.actions.iter().zip([10, 20, 20, 100, 101, 1, 1]) {
        block_on(adapter.schedule(&TimerSchedule {
            action: action.clone(),
            due_at: LogicalTime(due),
        }))
        .unwrap();
    }
    let claim_at = |now, limit| {
        block_on(
            adapter.claim_due(
                &TimerClaimRequest::new(
                    scope(),
                    id("pri_worker"),
                    LogicalTime(now),
                    NonZeroU16::new(limit).unwrap(),
                )
                .unwrap(),
            ),
        )
        .unwrap()
    };
    let completed = claim_at(1, 1);
    assert_eq!(completed[0].timer.action.action_id, id("act_e"));
    block_on(TimerClaimStore::acknowledge(
        &adapter,
        &completed[0],
        LogicalTime(1),
    ))
    .unwrap();
    block_on(adapter.cancel(&TimerSchedule {
        action: batch.actions[6].clone(),
        due_at: LogicalTime(1),
    }))
    .unwrap();
    for (now, limit, expected) in [
        (10, 1, vec!["act_z"]),
        (20, 1, vec!["act_a"]),
        (20, 10, vec!["act_b"]),
        (21, 2, vec!["act_z"]),
        (100, 2, vec!["act_z", "act_a"]),
        (100, 2, vec!["act_b", "act_c"]),
        (100, 10, vec![]),
    ] {
        let claimed = claim_at(now, limit);
        let identities: Vec<_> = claimed
            .iter()
            .map(|lease| lease.timer.action.action_id.as_str())
            .collect();
        assert_eq!(identities, expected, "now={now} limit={limit}");
    }
}

#[test]
fn indexed_outbox_skips_acknowledged_actions_and_unexpired_leases() {
    let directory = tempfile::tempdir().unwrap();
    let clock = clock();
    let adapter = store(&directory.path().join("outbox-order.db"), clock.clone());
    let mut batch = commit(false);
    let template = batch.actions[0].clone();
    batch.actions.clear();
    for name in ["a", "b", "c"] {
        let mut action = template.clone();
        action.action_id = id(&format!("act_{name}"));
        action.step_id = id(&format!("stp_{name}"));
        batch.actions.push(action);
    }
    block_on(ProcessStore::commit(&adapter, &batch)).unwrap();
    let request =
        OutboxClaimRequest::new(scope(), id("pri_worker"), NonZeroU16::new(1).unwrap()).unwrap();
    let completed = block_on(adapter.claim(&request)).unwrap();
    assert_eq!(completed[0].record.action.action_id, id("act_a"));
    block_on(OutboxStore::acknowledge(
        &adapter,
        &completed[0],
        LogicalTime(100),
    ))
    .unwrap();
    let live = block_on(adapter.claim(&request)).unwrap();
    assert_eq!(live[0].record.action.action_id, id("act_b"));
    let remaining = block_on(adapter.claim(&claim())).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].record.action.action_id, id("act_c"));
    clock.now.store(110, Ordering::SeqCst);
    assert!(block_on(adapter.claim(&claim())).unwrap().is_empty());
    clock.now.store(111, Ordering::SeqCst);
    let reclaimed = block_on(adapter.claim(&claim())).unwrap();
    let identities: Vec<_> = reclaimed
        .iter()
        .map(|lease| lease.record.action.action_id.as_str())
        .collect();
    assert_eq!(identities, ["act_b", "act_c"]);
}
