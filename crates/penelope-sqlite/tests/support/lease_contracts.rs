//! Durable lease failure paths: rejected operations leave work recoverable.
use super::*;

#[test]
fn cancellation_fences_a_live_timer_lease_and_rescheduling_cannot_resurrect_it() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("timers.db");
    let mut adapter = store(&path, clock());
    let commit = commit(true);
    let timer = TimerSchedule {
        action: commit.actions[0].clone(),
        due_at: LogicalTime(100),
    };
    assert_eq!(block_on(adapter.cancel(&timer)), Err(PortError::Conflict));
    block_on(ProcessStore::commit(&adapter, &commit)).unwrap();
    block_on(adapter.schedule(&timer)).unwrap();
    let lease = block_on(adapter.claim_due(&timer_claim(100)))
        .unwrap()
        .remove(0);
    let mut substituted = timer.clone();
    substituted.due_at = LogicalTime(101);
    assert_eq!(
        block_on(adapter.cancel(&substituted)),
        Err(PortError::Conflict)
    );
    assert_eq!(
        block_on(adapter.schedule(&substituted)),
        Err(PortError::Conflict)
    );
    block_on(adapter.cancel(&timer)).unwrap();
    assert_eq!(
        block_on(TimerClaimStore::acknowledge(
            &adapter,
            &lease,
            LogicalTime(100)
        )),
        Err(PortError::Conflict)
    );
    adapter = store(&path, clock());
    block_on(adapter.cancel(&timer)).unwrap();
    block_on(adapter.schedule(&timer)).unwrap();
    assert!(
        block_on(adapter.claim_due(&timer_claim(1000)))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn renewal_invalidates_the_old_envelope_and_survives_connection_reconstruction() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("outbox.db");
    let clock = clock();
    let adapter = store(&path, clock.clone());
    block_on(ProcessStore::commit(&adapter, &commit(false))).unwrap();
    let lease = block_on(adapter.claim(&claim())).unwrap().remove(0);
    clock.now.store(110, Ordering::SeqCst);
    assert!(block_on(adapter.claim(&claim())).unwrap().is_empty());
    // Expiry is inclusive: the holder may renew at its exact last valid tick.
    let renewed = block_on(adapter.renew(&lease, LogicalTime(110), LogicalTime(150))).unwrap();
    let reopened = store(&path, clock.clone());
    assert_eq!(
        block_on(reopened.renew(&lease, LogicalTime(110), LogicalTime(160))),
        Err(PortError::Conflict)
    );
    assert_eq!(
        block_on(OutboxStore::acknowledge(
            &reopened,
            &lease,
            LogicalTime(110)
        )),
        Err(PortError::Conflict)
    );
    clock.now.store(150, Ordering::SeqCst);
    assert!(block_on(reopened.claim(&claim())).unwrap().is_empty());
    block_on(OutboxStore::acknowledge(
        &reopened,
        &renewed,
        LogicalTime(150),
    ))
    .unwrap();
    assert_eq!(
        block_on(adapter.renew(&renewed, LogicalTime(150), LogicalTime(160))),
        Err(PortError::Conflict)
    );
    clock.now.store(151, Ordering::SeqCst);
    assert!(block_on(adapter.claim(&claim())).unwrap().is_empty());
}

#[test]
fn expired_or_nonextending_renewals_cannot_hide_pending_work() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("outbox.db");
    let clock = clock();
    let adapter = store(&path, clock.clone());
    block_on(ProcessStore::commit(&adapter, &commit(false))).unwrap();
    let lease = block_on(adapter.claim(&claim())).unwrap().remove(0);
    assert!(block_on(adapter.renew(&lease, LogicalTime(100), LogicalTime(109))).is_err());
    assert_eq!(
        block_on(adapter.renew(&lease, LogicalTime(111), LogicalTime(160))),
        Err(PortError::TimedOut)
    );
    clock.now.store(111, Ordering::SeqCst);
    let reclaimed = block_on(adapter.claim(&claim())).unwrap().remove(0);
    assert_ne!(lease.token, reclaimed.token);
    assert_eq!(reclaimed.record.delivery_attempt, 1);
}

#[test]
fn exhausted_tokens_and_delivery_attempts_roll_back_without_claiming_work() {
    for timer in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("leases.db");
        let adapter = store(&path, clock());
        let commit = commit(timer);
        block_on(ProcessStore::commit(&adapter, &commit)).unwrap();
        if timer {
            block_on(adapter.schedule(&TimerSchedule {
                action: commit.actions[0].clone(),
                due_at: LogicalTime(100),
            }))
            .unwrap();
        }
        let connection = Connection::open(&path).unwrap();
        connection
            .execute("UPDATE lease_counter SET next_token=?1", [i64::MAX])
            .unwrap();
        let token_exhausted = if timer {
            block_on(adapter.claim_due(&timer_claim(100))).map(|_| ())
        } else {
            block_on(adapter.claim(&claim())).map(|_| ())
        };
        assert_eq!(token_exhausted, Err(PortError::QuotaExceeded));
        let table = if timer {
            "process_timers"
        } else {
            "process_actions"
        };
        let state: (u32, Option<Vec<u8>>) = connection
            .query_row(&format!("SELECT attempt,lease FROM {table}"), [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!(state, (0, None));
        connection
            .execute("UPDATE lease_counter SET next_token=1", [])
            .unwrap();
        connection
            .execute(
                &format!("UPDATE {table} SET attempt=?1"),
                [MAX_OUTBOX_DELIVERY_ATTEMPTS],
            )
            .unwrap();
        let attempts_exhausted = if timer {
            block_on(adapter.claim_due(&timer_claim(100))).map(|_| ())
        } else {
            block_on(adapter.claim(&claim())).map(|_| ())
        };
        assert_eq!(attempts_exhausted, Err(PortError::QuotaExceeded));
        let counter: i64 = connection
            .query_row("SELECT next_token FROM lease_counter", [], |row| row.get(0))
            .unwrap();
        assert_eq!(counter, 1);
        // Operator repair of the artificial exhaustion restores exactly one
        // eligible delivery; neither rejected claim has consumed a token.
        connection
            .execute(&format!("UPDATE {table} SET attempt=0"), [])
            .unwrap();
        let length = if timer {
            block_on(adapter.claim_due(&timer_claim(100)))
                .unwrap()
                .len()
        } else {
            block_on(adapter.claim(&claim())).unwrap().len()
        };
        assert_eq!(length, 1);
    }
}

#[test]
fn corrupted_pending_records_block_claims_and_cancellation() {
    for timer in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("leases.db");
        let adapter = store(&path, clock());
        let commit = commit(timer);
        block_on(ProcessStore::commit(&adapter, &commit)).unwrap();
        let schedule = TimerSchedule {
            action: commit.actions[0].clone(),
            due_at: LogicalTime(100),
        };
        if timer {
            block_on(adapter.schedule(&schedule)).unwrap();
        }
        let connection = Connection::open(&path).unwrap();
        let table = if timer {
            "process_timers"
        } else {
            "process_actions"
        };
        connection
            .execute(&format!("UPDATE {table} SET checksum=zeroblob(32)"), [])
            .unwrap();
        let result = if timer {
            block_on(adapter.claim_due(&timer_claim(100))).map(|_| ())
        } else {
            block_on(adapter.claim(&claim())).map(|_| ())
        };
        assert_eq!(result, Err(PortError::Invariant));
        if timer {
            assert_eq!(
                block_on(adapter.cancel(&schedule)),
                Err(PortError::Invariant)
            );
        }
        let pending: bool = connection
            .query_row(&format!("SELECT lease IS NULL FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(pending);
    }
}

#[test]
fn clock_overflow_cannot_create_an_unbounded_lease() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("leases.db");
    let clock = clock();
    let adapter = store(&path, clock.clone());
    let commit = commit(true);
    block_on(ProcessStore::commit(&adapter, &commit)).unwrap();
    block_on(adapter.schedule(&TimerSchedule {
        action: commit.actions[0].clone(),
        due_at: LogicalTime(100),
    }))
    .unwrap();
    clock.now.store(u64::MAX, Ordering::SeqCst);
    assert_eq!(
        block_on(adapter.claim(&claim())),
        Err(PortError::QuotaExceeded)
    );
    assert_eq!(
        block_on(adapter.claim_due(&timer_claim(u64::MAX))),
        Err(PortError::QuotaExceeded)
    );
    clock.now.store(100, Ordering::SeqCst);
    assert_eq!(block_on(adapter.claim(&claim())).unwrap().len(), 1);
    assert_eq!(
        block_on(adapter.claim_due(&timer_claim(100)))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn damaged_index_bindings_never_return_an_unpersisted_lease() {
    for timer in [false, true] {
        for field in ["action_id", "identity"] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("index.db");
            let adapter = store(&path, clock());
            let mut commit = commit(timer);
            let mut other = commit.actions[0].clone();
            other.action_id = id("act_aaa");
            other.step_id = id("stp_other");
            commit.actions.push(other);
            block_on(ProcessStore::commit(&adapter, &commit)).unwrap();
            if timer {
                for action in &commit.actions {
                    block_on(adapter.schedule(&TimerSchedule {
                        action: action.clone(),
                        due_at: LogicalTime(100),
                    }))
                    .unwrap();
                }
            }
            let connection = Connection::open(&path).unwrap();
            let table = if timer {
                "process_timers"
            } else {
                "process_actions"
            };
            let damage = if field == "action_id" {
                format!("UPDATE {table} SET action_id='act_zzz' WHERE action_id='act_work'")
            } else if timer {
                format!("UPDATE {table} SET due_at=99 WHERE action_id='act_work'")
            } else {
                format!("UPDATE {table} SET effect_key=zeroblob(32) WHERE action_id='act_work'")
            };
            connection.execute(&damage, []).unwrap();
            let result = if timer {
                block_on(adapter.claim_due(&timer_claim(100))).map(|_| ())
            } else {
                block_on(adapter.claim(&claim())).map(|_| ())
            };
            assert_eq!(result, Err(PortError::Invariant), "{table}/{field}");
            let state: (u32, u32, i64) = connection.query_row(
                &format!("SELECT sum(attempt),sum(lease IS NULL),(SELECT next_token FROM lease_counter) FROM {table}"),
                [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            ).unwrap();
            assert_eq!(state, (0, 2, 1), "{table}/{field}");
        }
    }
}

#[test]
fn ignored_lease_update_rolls_back_tokens_and_returns_no_authority() {
    for timer in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ignored.db");
        let adapter = store(&path, clock());
        let commit = commit(timer);
        block_on(ProcessStore::commit(&adapter, &commit)).unwrap();
        if timer {
            block_on(adapter.schedule(&TimerSchedule {
                action: commit.actions[0].clone(),
                due_at: LogicalTime(100),
            }))
            .unwrap();
        }
        let table = if timer {
            "process_timers"
        } else {
            "process_actions"
        };
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch(&format!(
            "CREATE TRIGGER ignore_claim BEFORE UPDATE OF lease ON {table} BEGIN SELECT RAISE(IGNORE); END;"
        )).unwrap();
        let result = if timer {
            block_on(adapter.claim_due(&timer_claim(100))).map(|_| ())
        } else {
            block_on(adapter.claim(&claim())).map(|_| ())
        };
        assert_eq!(result, Err(PortError::Invariant));
        let state: (u32, bool, i64) = connection.query_row(
            &format!("SELECT attempt,lease IS NULL,(SELECT next_token FROM lease_counter) FROM {table}"),
            [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).unwrap();
        assert_eq!(state, (0, true, 1));
        connection
            .execute_batch("DROP TRIGGER ignore_claim;")
            .unwrap();
        let recovered_token = if timer {
            block_on(adapter.claim_due(&timer_claim(100)))
                .unwrap()
                .remove(0)
                .token
        } else {
            block_on(adapter.claim(&claim())).unwrap().remove(0).token
        };
        assert_eq!(
            recovered_token,
            OutboxLeaseToken::new(std::num::NonZeroU64::new(1).unwrap())
        );
    }
}
