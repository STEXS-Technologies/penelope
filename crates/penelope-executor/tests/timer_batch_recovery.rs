//! Real durable partial-batch timer delivery, reopen and exactly-once consumption.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing
)]
#[path = "support/timer_batch_faults.rs"]
mod faults;
#[path = "support/timer_input_legacy.rs"]
mod legacy;
use faults::{Clock, Fault, Ports, run};
use penelope_domain::*;
use penelope_executor::services::drain_timers;
use penelope_ports::{inbox::PendingInbox, payloads::ProcessPayloadStore, *};
use penelope_sqlite::{SqliteLimits, SqliteProcessStore};
use rusqlite::{Connection, types::Value};
use std::{
    num::{NonZeroU16, NonZeroU64},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
fn open(path: &Path, clock: &Arc<Clock>) -> SqliteProcessStore {
    SqliteProcessStore::open(path, SqliteLimits::default(), clock.clone(), 10, 16).unwrap()
}
fn setup(
    store: &SqliteProcessStore,
    width: usize,
    batch: usize,
) -> (TimerClaimRequest, Vec<TimerSchedule>, Vec<ProcessOutcome>) {
    let suffix = "x".repeat(width - 4);
    let scope = ProcessScope::new(
        format!("tnt_{suffix}").parse().unwrap(),
        format!("prc_{suffix}").parse().unwrap(),
        format!("def_{suffix}").parse().unwrap(),
        format!("dfv_{suffix}").parse().unwrap(),
        ContentDigest([3; 32]),
    );
    let timers: Vec<_> = (0..batch)
        .map(|index| TimerSchedule {
            action: ProcessAction::new(
                scope.clone(),
                format!("act_{}{:04}", "x".repeat(width - 8), index)
                    .parse()
                    .unwrap(),
                format!("stp_{index:03}").parse().unwrap(),
                0,
                ProcessActionKind::Timer,
                ContentDigest([4; 32]),
            ),
            due_at: LogicalTime(100),
        })
        .collect();
    let input = ProcessInput::new(
        scope.tenant_id.clone(),
        scope.process_id.clone(),
        "inp_start".parse().unwrap(),
        ProcessInputKind::CanonicalEvent,
        ContentDigest([5; 32]),
    );
    let outcome = ProcessOutcome::new(
        scope.clone(),
        0,
        ProcessOutcomeFact::new(
            "out_start".parse().unwrap(),
            CausationId::Input(input.input_id.clone()),
            OutcomeActor::System,
            LogicalTime(100),
            ProcessOutcomeKind::Started,
            ContentDigest([6; 32]),
        ),
    );
    let commit = AtomicProcessCommit::new(
        0,
        Some(input),
        vec![outcome.clone()],
        timers.iter().map(|timer| timer.action.clone()).collect(),
    )
    .unwrap();
    let receipt = run(store.commit(&commit)).unwrap();
    receipt.validate_for(&commit).unwrap();
    for timer in &timers {
        run(store.schedule(timer)).unwrap();
    }
    (
        TimerClaimRequest::new(
            scope,
            "pri_initial".parse().unwrap(),
            LogicalTime(u64::MAX),
            NonZeroU16::new(u16::try_from(batch).unwrap()).unwrap(),
        )
        .unwrap(),
        timers,
        vec![outcome],
    )
}
fn snapshot(path: &Path) -> Vec<(String, Vec<Vec<Value>>)> {
    let connection = Connection::open(path).unwrap();
    [
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
    ]
    .into_iter()
    .map(|table| {
        let mut statement = connection
            .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .unwrap();
        let columns = statement.column_count();
        let rows = statement
            .query_map([], |row| {
                (0..columns)
                    .map(|index| row.get::<_, Value>(index))
                    .collect()
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        (table.into(), rows)
    })
    .collect()
}
fn pending(path: &Path) -> Vec<ProcessInput> {
    let connection = Connection::open(path).unwrap();
    let mut statement=connection.prepare("SELECT input,checksum FROM process_inbox WHERE committed_through IS NULL ORDER BY input_id").unwrap();
    statement
        .query_map([], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .unwrap()
        .map(|row| {
            let (bytes, digest) = row.unwrap();
            assert_eq!(ContentDigest::sha256(&bytes).0.as_slice(), digest);
            serde_json::from_slice(&bytes).unwrap()
        })
        .collect()
}
fn check_pending(
    store: &SqliteProcessStore,
    path: &Path,
    request: &TimerClaimRequest,
    expected: &[ProcessInput],
) {
    let mut expected = expected.to_vec();
    expected.sort_by(|left, right| left.input_id.as_str().cmp(right.input_id.as_str()));
    assert_eq!(pending(path), expected);
    let discovered = run(store.pending_inputs(
        &request.scope.tenant_id,
        &request.scope.process_id,
        NonZeroU16::new(64).unwrap(),
    ))
    .unwrap();
    assert_eq!(
        discovered,
        expected.into_iter().take(64).collect::<Vec<_>>()
    );
    assert_eq!(
        run(store.pending_inputs(
            &request.scope.tenant_id,
            &request.scope.process_id,
            NonZeroU16::new(64).unwrap()
        ))
        .unwrap(),
        discovered
    );
}
fn check_timers(
    path: &Path,
    original: &[TimerLease],
    replacement: &[TimerLease],
    acknowledged: usize,
    final_state: bool,
) {
    let connection = Connection::open(path).unwrap();
    let mut statement=connection.prepare("SELECT timer,checksum,state,attempt,lease,expires_at,due_at FROM process_timers ORDER BY action_id").unwrap();
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, u8>(2)?,
                row.get::<_, u32>(3)?,
                row.get::<_, Vec<u8>>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(rows.len(), original.len());
    for (index, (bytes, digest, state, attempt, lease, expires, due)) in
        rows.into_iter().enumerate()
    {
        assert_eq!(ContentDigest::sha256(&bytes).0.as_slice(), digest);
        assert_eq!(
            serde_json::from_slice::<TimerSchedule>(&bytes).unwrap(),
            original[index].timer
        );
        let expected = if final_state && index >= acknowledged {
            &replacement[index - acknowledged]
        } else {
            &original[index]
        };
        assert_eq!(
            serde_json::from_slice::<TimerLease>(&lease).unwrap(),
            *expected
        );
        assert_eq!(expires, i64::try_from(expected.lease_expires_at.0).unwrap());
        assert_eq!(due, 100);
        assert_eq!(state, u8::from(final_state || index < acknowledged));
        assert_eq!(
            attempt,
            if final_state && index >= acknowledged {
                2
            } else {
                1
            }
        );
    }
    let next: i64 = connection
        .query_row(
            "SELECT next_token FROM lease_counter WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        next,
        i64::try_from(1 + original.len() + replacement.len()).unwrap()
    );
}
fn payload_count(path: &Path) -> usize {
    let count: i64 = Connection::open(path)
        .unwrap()
        .query_row("SELECT count(*) FROM process_payloads", [], |row| {
            row.get(0)
        })
        .unwrap();
    usize::try_from(count).unwrap()
}

fn scenario(width: usize, batch: usize, at: usize, fault: Fault) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("timers.db");
    let clock = Arc::new(Clock(AtomicU64::new(100)));
    let initial_store = open(&path, &clock);
    let (mut request, timers, mut outcomes) = setup(&initial_store, width, batch);
    let ports = Ports::new(
        &initial_store,
        &clock,
        &request,
        Some((fault, u64::try_from(at).unwrap())),
    );
    let failure = run(drain_timers(&ports, &ports, &ports, &request, &ports)).unwrap_err();
    assert!(ports.fired.load(Ordering::SeqCst));
    assert_eq!(failure.stage, fault.stage());
    assert_eq!(
        failure.source,
        if fault == Fault::ReceiptMismatch {
            PortError::Invariant
        } else {
            PortError::Unavailable
        }
    );
    assert_eq!(usize::from(failure.report.claimed), batch);
    assert_eq!(usize::from(failure.report.acknowledged), at);
    assert_eq!(failure.report.duplicate_inputs, 0);
    let leases = ports.leases.lock().unwrap().clone();
    assert_eq!(leases.len(), batch);
    for (index, (lease, timer)) in leases.iter().zip(&timers).enumerate() {
        assert_eq!(&lease.timer, timer);
        assert_eq!(lease.owner, request.owner);
        assert_eq!(lease.lease_expires_at, LogicalTime(110));
        assert_eq!(
            lease.token,
            OutboxLeaseToken::new(NonZeroU64::new(u64::try_from(index + 1).unwrap()).unwrap())
        );
    }
    let expected: Vec<_> = leases
        .iter()
        .map(|lease| legacy::timer_input(lease).unwrap())
        .collect();
    let inputs: Vec<_> = expected.iter().map(|(input, _)| input.clone()).collect();
    let accepted = at + usize::from(fault.accepted());
    let acknowledged = at + usize::from(fault == Fault::AckAfter);
    let payloads = at + usize::from(fault != Fault::PayloadBefore);
    check_pending(&initial_store, &path, &request, &inputs[..accepted]);
    check_timers(&path, &leases, &[], acknowledged, false);
    assert_eq!(payload_count(&path), payloads);
    for (input, bytes) in expected.iter().take(payloads) {
        assert_eq!(
            run(initial_store.read_payload(&request.scope, input.payload_digest)).unwrap(),
            *bytes
        );
    }
    assert_eq!(
        ports.payloads.load(Ordering::SeqCst),
        u64::try_from(at + 1).unwrap()
    );
    let accepts = at + usize::from(!matches!(fault, Fault::PayloadBefore | Fault::PayloadAfter));
    assert_eq!(
        ports.accepts.load(Ordering::SeqCst),
        u64::try_from(accepts).unwrap()
    );
    let acks = at + usize::from(matches!(fault, Fault::AckBefore | Fault::AckAfter));
    assert_eq!(
        ports.acknowledgements.load(Ordering::SeqCst),
        u64::try_from(acks).unwrap()
    );
    drop(ports);
    drop(initial_store);
    let failed_snapshot = snapshot(&path);
    clock.0.store(110, Ordering::SeqCst);
    let reopened_store = open(&path, &clock);
    request.owner = "pri_recovery".parse().unwrap();
    let held = Ports::new(&reopened_store, &clock, &request, None);
    let held_report = run(drain_timers(&held, &held, &held, &request, &held)).unwrap();
    assert_eq!(held_report.claimed, 0);
    assert_eq!(held_report.acknowledged, 0);
    assert_eq!(held_report.duplicate_inputs, 0);
    assert_eq!(snapshot(&path), failed_snapshot);
    drop(held);
    clock.0.store(111, Ordering::SeqCst);
    let recovered = Ports::new(&reopened_store, &clock, &request, None);
    let recovered_report = run(drain_timers(
        &recovered, &recovered, &recovered, &request, &recovered,
    ))
    .unwrap();
    assert_eq!(usize::from(recovered_report.claimed), batch - acknowledged);
    assert_eq!(
        usize::from(recovered_report.acknowledged),
        batch - acknowledged
    );
    assert_eq!(
        usize::from(recovered_report.duplicate_inputs),
        accepted - acknowledged
    );
    let replacement = recovered.leases.lock().unwrap().clone();
    assert_eq!(replacement.len(), batch - acknowledged);
    for (index, lease) in replacement.iter().enumerate() {
        assert_eq!(lease.owner, request.owner);
        assert_eq!(lease.lease_expires_at, LogicalTime(121));
        assert_eq!(
            lease.token,
            OutboxLeaseToken::new(
                NonZeroU64::new(u64::try_from(batch + index + 1).unwrap()).unwrap()
            )
        );
        assert_eq!(
            legacy::timer_input(lease).unwrap(),
            expected[acknowledged + index]
        );
    }
    check_timers(&path, &leases, &replacement, acknowledged, true);
    check_pending(&reopened_store, &path, &request, &inputs);
    assert_eq!(payload_count(&path), batch);
    for (input, bytes) in &expected {
        assert_eq!(
            run(reopened_store.read_payload(&request.scope, input.payload_digest)).unwrap(),
            *bytes
        );
    }
    drop(recovered);
    // Timer ACK queues inputs; only these process commits consume them.
    let mut commits = Vec::new();
    let mut ordered = inputs;
    ordered.sort_by(|left, right| left.input_id.as_str().cmp(right.input_id.as_str()));
    for chunk in ordered.chunks(64) {
        let discovered = run(reopened_store.pending_inputs(
            &request.scope.tenant_id,
            &request.scope.process_id,
            NonZeroU16::new(64).unwrap(),
        ))
        .unwrap();
        assert_eq!(discovered, chunk);
        for input in &discovered {
            let index = commits.len();
            let sequence = u64::try_from(index + 1).unwrap();
            let outcome = ProcessOutcome::new(
                request.scope.clone(),
                sequence,
                ProcessOutcomeFact::new(
                    format!("out_timer_{index:03}").parse().unwrap(),
                    CausationId::Input(input.input_id.clone()),
                    OutcomeActor::System,
                    LogicalTime(111),
                    ProcessOutcomeKind::TimerFired,
                    input.payload_digest,
                ),
            );
            let commit = AtomicProcessCommit::new(
                sequence,
                Some(input.clone()),
                vec![outcome.clone()],
                vec![],
            )
            .unwrap();
            let receipt = run(reopened_store.commit(&commit)).unwrap();
            receipt.validate_for(&commit).unwrap();
            assert!(receipt.duplicate_input);
            outcomes.push(outcome);
            commits.push(commit);
        }
    }
    check_pending(&reopened_store, &path, &request, &[]);
    let consumed_snapshot = snapshot(&path);
    for commit in &commits {
        let receipt = run(reopened_store.commit(commit)).unwrap();
        receipt.validate_for(commit).unwrap();
        assert!(receipt.duplicate_input);
    }
    assert_eq!(snapshot(&path), consumed_snapshot);
    drop(reopened_store);
    let verified_store = open(&path, &clock);
    assert_eq!(snapshot(&path), consumed_snapshot);
    check_pending(&verified_store, &path, &request, &[]);
    let page = run(verified_store.read_outcomes(
        &OutcomeReplayRequest::new(request.scope.clone(), 0, NonZeroU16::new(512).unwrap())
            .unwrap(),
    ))
    .unwrap();
    page.validate().unwrap();
    assert_eq!(page.outcomes, outcomes);
    assert_eq!(page.next_sequence, None);
    let replay = Ports::new(&verified_store, &clock, &request, None);
    assert_eq!(
        run(drain_timers(&replay, &replay, &replay, &request, &replay))
            .unwrap()
            .claimed,
        0
    );
    drop(replay);
    let sequence = u64::try_from(batch + 1).unwrap();
    let input = ordered[0].clone();
    let conflict = AtomicProcessCommit::new(
        sequence,
        Some(input.clone()),
        vec![ProcessOutcome::new(
            request.scope.clone(),
            sequence,
            ProcessOutcomeFact::new(
                "out_reconsume".parse().unwrap(),
                CausationId::Input(input.input_id),
                OutcomeActor::System,
                LogicalTime(111),
                ProcessOutcomeKind::TimerFired,
                input.payload_digest,
            ),
        )],
        vec![],
    )
    .unwrap();
    assert_eq!(
        run(verified_store.commit(&conflict)),
        Err(PortError::Conflict)
    );
    assert_eq!(snapshot(&path), consumed_snapshot);
}
#[test]
fn durable_timer_batches_recover_partial_delivery_without_reconsuming_inputs() {
    let filter = std::env::var("PENELOPE_TIMER_BATCH_CASE").ok();
    let mut cases = 0;
    for (width, batch) in [(9, 17), (128, 128)] {
        for at in [0, batch / 2, batch - 1] {
            for fault in Fault::ALL {
                let name = format!("{batch}-{width}-{at}-{fault:?}");
                if filter.as_ref().is_some_and(|expected| expected != &name) {
                    continue;
                }
                scenario(width, batch, at, fault);
                cases += 1;
                println!("timer_batch_case={name} verified=true");
            }
        }
    }
    assert_eq!(cases, if filter.is_some() { 1 } else { 48 });
    println!(
        "timer_batch_recovery_cases={cases} durable_reopen=true stable_inputs=true exact_partial_reports=true consume_once=true public_discovery=true"
    );
}
