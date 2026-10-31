//! Inconsistent head/tail metadata cannot acknowledge an authoritative operation.
use super::write_snapshot::{Snapshot, snapshot};
use super::*;
const TABLES: &[&str] = &[
    "process_usage",
    "process_inbox",
    "process_sources",
    "process_outcomes",
    "process_actions",
    "process_heads",
    "process_commits",
    "process_payloads",
    "process_timers",
    "lease_counter",
];
#[derive(Clone, Copy, Debug)]
enum Operation {
    Read(u64),
    Retry,
    Append,
}
fn batch() -> AtomicProcessCommit {
    let mut batch = commit(false).with_canonical_source_event(id("cev_frontier"));
    let mut second = batch.outcomes[0].clone();
    second.sequence = 1;
    second.outcome_id = id("out_second");
    batch.outcomes.push(second);
    batch
}
fn follow_up(first: &AtomicProcessCommit) -> AtomicProcessCommit {
    let mut outcome = first.outcomes[0].clone();
    outcome.sequence = 2;
    outcome.outcome_id = id("out_followup");
    outcome.kind = ProcessOutcomeKind::ActionPlanned;
    AtomicProcessCommit::new(2, None, vec![outcome], Vec::new()).unwrap()
}
fn invoke(
    adapter: &SqliteProcessStore,
    first: &AtomicProcessCommit,
    next: &AtomicProcessCommit,
    operation: Operation,
) {
    match operation {
        Operation::Read(from) => {
            let request =
                OutcomeReplayRequest::new(scope(), from, NonZeroU16::new(1).unwrap()).unwrap();
            let page = block_on(adapter.read_outcomes(&request)).unwrap();
            {
                page.validate().unwrap();
                assert_eq!(
                    page.outcomes,
                    first
                        .outcomes
                        .iter()
                        .filter(|outcome| outcome.sequence == from)
                        .cloned()
                        .collect::<Vec<_>>()
                );
                assert_eq!(page.next_sequence, if from == 0 { Some(1) } else { None });
            }
        }
        Operation::Retry => {
            let receipt = block_on(ProcessStore::commit(adapter, first)).unwrap();
            receipt.validate_for(first).unwrap();
            assert!(receipt.duplicate_input);
        }
        Operation::Append => {
            let receipt = block_on(ProcessStore::commit(adapter, next)).unwrap();
            receipt.validate_for(next).unwrap();
            assert!(!receipt.duplicate_input);
        }
    }
}
fn restore(connection: &mut Connection, saved: &Snapshot) {
    let transaction = connection.transaction().unwrap();
    for (table, rows) in saved {
        transaction
            .execute(&format!("DELETE FROM {table}"), [])
            .unwrap();
        for row in rows {
            let placeholders = std::iter::repeat_n("?", row.len())
                .collect::<Vec<_>>()
                .join(",");
            transaction
                .execute(
                    &format!("INSERT INTO {table} VALUES ({placeholders})"),
                    rusqlite::params_from_iter(row),
                )
                .unwrap();
        }
    }
    transaction.commit().unwrap();
}
#[test]
fn inconsistent_frontiers_reject_pages_retries_and_appends_until_repaired() {
    let mut failures = Vec::new();
    for damage in [
        "missing-head",
        "lowered-head",
        "advanced-head",
        "missing-tail",
        "missing-all",
        "extra-tail",
    ] {
        for operation in [
            Operation::Read(0),
            Operation::Read(1),
            Operation::Read(2),
            Operation::Read(99),
            Operation::Retry,
            Operation::Append,
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("frontier.db");
            let adapter = store(&path, clock());
            let first = batch();
            let next = follow_up(&first);
            block_on(ProcessStore::commit(&adapter, &first)).unwrap();
            let mut connection = Connection::open(&path).unwrap();
            let intact = snapshot(&connection, TABLES);
            match damage {
                "missing-head" => {
                    connection.execute("DELETE FROM process_heads", []).unwrap();
                }
                "lowered-head" => {
                    connection
                        .execute("UPDATE process_heads SET next_sequence=1", [])
                        .unwrap();
                }
                "advanced-head" => {
                    connection
                        .execute("UPDATE process_heads SET next_sequence=3", [])
                        .unwrap();
                }
                "missing-tail" => {
                    connection
                        .execute("DELETE FROM process_outcomes WHERE sequence=1", [])
                        .unwrap();
                }
                "missing-all" => {
                    connection
                        .execute("DELETE FROM process_outcomes", [])
                        .unwrap();
                }
                _ => {
                    let outcome = &next.outcomes[0];
                    let bytes = serde_json::to_vec(outcome).unwrap();
                    connection
                        .execute(
                            "INSERT INTO process_outcomes VALUES (?1,?2,?3,?4,?5)",
                            rusqlite::params![
                                outcome.scope().canonical_bytes(),
                                2,
                                outcome.outcome_id.as_str(),
                                bytes,
                                ContentDigest::sha256(&bytes).0.as_slice()
                            ],
                        )
                        .unwrap();
                }
            }
            let damaged = snapshot(&connection, TABLES);
            // Baseline false successes can have invalid continuation; classify
            // raw results here before checking healthy-page expectations below.
            let result = match operation {
                Operation::Read(from) => block_on(adapter.read_outcomes(
                    &OutcomeReplayRequest::new(scope(), from, NonZeroU16::new(1).unwrap()).unwrap(),
                ))
                .map(|_page| ()),
                Operation::Retry => {
                    block_on(ProcessStore::commit(&adapter, &first)).map(|_receipt| ())
                }
                Operation::Append => {
                    block_on(ProcessStore::commit(&adapter, &next)).map(|_receipt| ())
                }
            };
            let unchanged = snapshot(&connection, TABLES) == damaged;
            println!(
                "damage={damage} operation={operation:?} result={result:?} unchanged={unchanged}"
            );
            if result != Err(PortError::Invariant) || !unchanged {
                failures.push(format!(
                    "{damage}/{operation:?}: {result:?}, unchanged={unchanged}"
                ));
                continue;
            }
            restore(&mut connection, &intact);
            assert_eq!(snapshot(&connection, TABLES), intact);
            invoke(&adapter, &first, &next, operation);
            drop(adapter);
            let reopened = store(&path, clock());
            block_on(ProcessStore::commit(&reopened, &first))
                .unwrap()
                .validate_for(&first)
                .unwrap();
            let page = block_on(reopened.read_outcomes(&replay())).unwrap();
            let mut expected = first.outcomes.clone();
            if matches!(operation, Operation::Append) {
                expected.extend(next.outcomes.clone());
                block_on(ProcessStore::commit(&reopened, &next))
                    .unwrap()
                    .validate_for(&next)
                    .unwrap();
            }
            assert_eq!(page.outcomes, expected);
        }
    }
    assert!(
        failures.is_empty(),
        "inconsistent frontier accepted or misclassified: {failures:?}"
    );
}
#[test]
fn unknown_and_inbox_only_scopes_remain_empty_with_other_history_present() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("scopes.db");
    let adapter = store(&path, clock());
    block_on(ProcessStore::commit(&adapter, &batch())).unwrap();
    for accepted in [false, true] {
        let mut other = scope();
        other.process_id = if accepted {
            id("prc_pending")
        } else {
            id("prc_unknown")
        };
        if accepted {
            let input = ProcessInput::new(
                other.tenant_id.clone(),
                other.process_id.clone(),
                id("inp_pending"),
                ProcessInputKind::CanonicalEvent,
                ContentDigest([5; 32]),
            );
            block_on(adapter.accept(&input)).unwrap();
        }
        for from in [0, 1, 99] {
            let page = block_on(
                adapter.read_outcomes(
                    &OutcomeReplayRequest::new(other.clone(), from, NonZeroU16::new(1).unwrap())
                        .unwrap(),
                ),
            )
            .unwrap();
            assert!(page.outcomes.is_empty());
            assert_eq!(page.next_sequence, None);
            assert_eq!(page.scope, other);
        }
    }
}

/// Engine-only work check with synthetic rows; this does not measure port latency.
#[test]
fn frontier_query_uses_scoped_indexes_as_history_grows() {
    let source = include_str!("../../src/process.rs");
    let query = source
        .split("fn process_head(")
        .nth(1)
        .unwrap()
        .split(".prepare_cached(\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("indexed-frontier.db");
    let adapter = store(&path, clock());
    let mut connection = Connection::open(&path).unwrap();
    // Keep another scope larger than each selected history.
    let other = b"unrelated".as_slice();
    let selected = scope().canonical_bytes();
    let seed_transaction = connection.transaction().unwrap();
    for sequence in 0..8192 {
        seed_transaction
            .execute(
                "INSERT INTO process_outcomes VALUES (?1,?2,?3,?4,?5)",
                rusqlite::params![
                    other,
                    sequence,
                    format!("other_{sequence}"),
                    b"synthetic".as_slice(),
                    b"digest".as_slice()
                ],
            )
            .unwrap();
    }
    seed_transaction.commit().unwrap();
    let mut work = Vec::new();
    for count in [1, 128, 4096] {
        let transaction = connection.transaction().unwrap();
        transaction
            .execute("DELETE FROM process_heads", [])
            .unwrap();
        transaction
            .execute("DELETE FROM process_outcomes WHERE scope=?1", [&selected])
            .unwrap();
        transaction
            .execute(
                "INSERT INTO process_heads VALUES (?1,?2,?3,0)",
                rusqlite::params![selected, b"selected".as_slice(), count],
            )
            .unwrap();
        for sequence in 0..count {
            transaction
                .execute(
                    "INSERT INTO process_outcomes VALUES (?1,?2,?3,?4,?5)",
                    rusqlite::params![
                        selected,
                        sequence,
                        format!("selected_{sequence}"),
                        b"synthetic".as_slice(),
                        b"digest".as_slice()
                    ],
                )
                .unwrap();
        }
        transaction.commit().unwrap();
        let mut statement = connection.prepare(query).unwrap();
        let frontier: (i64, i64, i64) = statement
            .query_row([&selected], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap();
        assert_eq!(frontier, (count, 0, count - 1));
        let steps = statement.get_status(rusqlite::StatementStatus::VmStep);
        let sorts = statement.get_status(rusqlite::StatementStatus::Sort);
        let plan = connection
            .prepare(&format!("EXPLAIN QUERY PLAN {query}"))
            .unwrap()
            .query_map([&selected], |row| row.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        println!(
            "sqlite={} history={count} unrelated=8192 vm_steps={steps} sorts={sorts} plan={plan:?}",
            rusqlite::version()
        );
        assert_eq!(sorts, 0);
        assert!(
            plan.iter()
                .any(|line| line.contains("SEARCH process_outcomes USING COVERING INDEX"))
        );
        work.push(steps);
    }
    assert_eq!(work[0], work[1]);
    assert_eq!(work[1], work[2]);
    drop(adapter);
}
