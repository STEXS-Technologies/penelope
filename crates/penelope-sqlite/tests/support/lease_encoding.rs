//! A reused buffer must persist each lease independently, including shorter JSON.
use super::*;
#[test]
fn variable_length_lease_bytes_remain_exact_across_reclaim_and_reopen() {
    for timers in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("lease-encoding.db");
        let clock = clock();
        let adapter = store(&path, clock.clone());
        let mut committed = commit(timers);
        committed.actions[0].action_id = id("act_aaa");
        committed.actions[0].step_id = id(&format!("stp_{}", "a".repeat(120)));
        let mut shorter = committed.actions[0].clone();
        shorter.action_id = id("act_zzz");
        shorter.step_id = id("stp_z");
        committed.actions.push(shorter);
        block_on(ProcessStore::commit(&adapter, &committed)).unwrap();
        if timers {
            for action in &committed.actions {
                block_on(adapter.schedule(&TimerSchedule {
                    action: action.clone(),
                    due_at: LogicalTime(1),
                }))
                .unwrap();
            }
        }
        let connection = Connection::open(&path).unwrap();
        connection
            .execute("UPDATE lease_counter SET next_token=99", [])
            .unwrap();
        let table = if timers {
            "process_timers"
        } else {
            "process_actions"
        };
        let mut old: Vec<(String, Vec<u8>)> = Vec::new();
        for (round, now, owner) in [
            (0_u64, 100, format!("pri_{}", "w".repeat(120))),
            (1, 111, "pri_x".to_owned()),
        ] {
            clock.now.store(now, Ordering::SeqCst);
            let expected: Vec<(String, Vec<u8>)> = if timers {
                let request = TimerClaimRequest::new(
                    scope(),
                    id(&owner),
                    LogicalTime(now),
                    NonZeroU16::new(2).unwrap(),
                )
                .unwrap();
                let leases = block_on(adapter.claim_due(&request)).unwrap();
                for (index, lease) in leases.iter().enumerate() {
                    assert_eq!(lease.owner, request.owner);
                    assert_eq!(
                        lease.token,
                        OutboxLeaseToken::new(
                            std::num::NonZeroU64::new(
                                99 + round * 2 + u64::try_from(index).unwrap()
                            )
                            .unwrap()
                        )
                    );
                }
                leases
                    .iter()
                    .map(|lease| {
                        (
                            lease.timer.action.action_id.as_str().to_owned(),
                            serde_json::to_vec(lease).unwrap(),
                        )
                    })
                    .collect()
            } else {
                let request =
                    OutboxClaimRequest::new(scope(), id(&owner), NonZeroU16::new(2).unwrap())
                        .unwrap();
                let leases = block_on(adapter.claim(&request)).unwrap();
                for (index, lease) in leases.iter().enumerate() {
                    assert_eq!(lease.owner, request.owner);
                    assert_eq!(lease.record.delivery_attempt, u32::try_from(round).unwrap());
                    assert_eq!(
                        lease.token,
                        OutboxLeaseToken::new(
                            std::num::NonZeroU64::new(
                                99 + round * 2 + u64::try_from(index).unwrap()
                            )
                            .unwrap()
                        )
                    );
                }
                leases
                    .iter()
                    .map(|lease| {
                        (
                            lease.record.action.action_id.as_str().to_owned(),
                            serde_json::to_vec(lease).unwrap(),
                        )
                    })
                    .collect()
            };
            assert_eq!(expected.len(), 2);
            assert!(expected[0].1.len() > expected[1].1.len());
            let persisted: Vec<(String, Vec<u8>)> = connection
                .prepare(&format!(
                    "SELECT action_id,lease FROM {table} ORDER BY action_id"
                ))
                .unwrap()
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            assert_eq!(persisted, expected);
            for (_, bytes) in &old {
                let result = if timers {
                    let lease: TimerLease = serde_json::from_slice(bytes).unwrap();
                    block_on(TimerClaimStore::acknowledge(
                        &adapter,
                        &lease,
                        LogicalTime(110),
                    ))
                } else {
                    let lease: OutboxLease = serde_json::from_slice(bytes).unwrap();
                    block_on(OutboxStore::acknowledge(&adapter, &lease, LogicalTime(110)))
                };
                assert_eq!(result, Err(PortError::Conflict));
            }
            old = expected;
        }
        // Reopen before ACK: exact stored authority must survive connection loss.
        drop(adapter);
        let reopened = store(&path, clock);
        for (_, bytes) in &old {
            if timers {
                let lease: TimerLease = serde_json::from_slice(bytes).unwrap();
                block_on(TimerClaimStore::acknowledge(
                    &reopened,
                    &lease,
                    LogicalTime(111),
                ))
                .unwrap();
            } else {
                let lease: OutboxLease = serde_json::from_slice(bytes).unwrap();
                block_on(OutboxStore::acknowledge(
                    &reopened,
                    &lease,
                    LogicalTime(111),
                ))
                .unwrap();
            }
        }
        let terminal = if timers { "state=1" } else { "acknowledged=1" };
        let count: u32 = connection
            .query_row(
                &format!("SELECT count(*) FROM {table} WHERE {terminal}"),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
        let next: i64 = connection
            .query_row("SELECT next_token FROM lease_counter", [], |row| row.get(0))
            .unwrap();
        assert_eq!(next, 103);
        println!(
            "timers={timers} variable_lengths=true stale_rejected=true reopened_ack=2 next_token=103"
        );
    }
}
