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

#[test]
fn lease_completion_rejects_damaged_durable_evidence() {
    for timer in [false, true] {
        for field in [
            "checksum",
            "payload",
            "expiry",
            "missing-expiry",
            "identity",
            "attempt",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("ack-evidence.db");
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
            let outbox = if timer {
                None
            } else {
                Some(block_on(adapter.claim(&claim())).unwrap().remove(0))
            };
            let timer_lease = if timer {
                Some(
                    block_on(adapter.claim_due(&timer_claim(100)))
                        .unwrap()
                        .remove(0),
                )
            } else {
                None
            };
            let table = if timer {
                "process_timers"
            } else {
                "process_actions"
            };
            let connection = Connection::open(&path).unwrap();
            match field {
                "checksum" => {
                    connection
                        .execute(&format!("UPDATE {table} SET checksum=zeroblob(32)"), [])
                        .unwrap();
                }
                "payload" => {
                    let (column, bytes) = if timer {
                        let mut other = schedule.clone();
                        other.action.payload_digest = ContentDigest([7; 32]);
                        ("timer", serde_json::to_vec(&other).unwrap())
                    } else {
                        let mut other = commit.actions[0].clone();
                        other.payload_digest = ContentDigest([7; 32]);
                        ("action", serde_json::to_vec(&other).unwrap())
                    };
                    connection
                        .execute(
                            &format!("UPDATE {table} SET {column}=?1,checksum=?2"),
                            rusqlite::params![bytes, ContentDigest::sha256(&bytes).0.as_slice()],
                        )
                        .unwrap();
                }
                "expiry" => {
                    connection
                        .execute(&format!("UPDATE {table} SET expires_at=109"), [])
                        .unwrap();
                }
                "missing-expiry" => {
                    connection
                        .execute(&format!("UPDATE {table} SET expires_at=NULL"), [])
                        .unwrap();
                }
                "identity" if timer => {
                    connection
                        .execute("UPDATE process_timers SET due_at=99", [])
                        .unwrap();
                }
                "identity" => {
                    connection
                        .execute("UPDATE process_actions SET effect_key=zeroblob(32)", [])
                        .unwrap();
                }
                _ => {
                    connection
                        .execute(&format!("UPDATE {table} SET attempt=0"), [])
                        .unwrap();
                }
            }
            if let Some(lease) = timer_lease {
                assert_eq!(
                    block_on(TimerClaimStore::acknowledge(
                        &adapter,
                        &lease,
                        LogicalTime(100)
                    )),
                    Err(PortError::Invariant),
                    "{table}/{field}"
                );
            }
            if let Some(lease) = outbox {
                assert_eq!(
                    block_on(OutboxStore::acknowledge(&adapter, &lease, LogicalTime(100))),
                    Err(PortError::Invariant),
                    "{table}/{field}"
                );
                assert_eq!(
                    block_on(adapter.renew(&lease, LogicalTime(100), LogicalTime(150))),
                    Err(PortError::Invariant),
                    "{table}/{field}/renew"
                );
            }
            let terminal: u32 = connection
                .query_row(
                    &format!(
                        "SELECT {} FROM {table}",
                        if timer { "state" } else { "acknowledged" }
                    ),
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(terminal, 0);
        }
    }
}

#[test]
fn ignored_acknowledgement_is_not_reported_as_durable_success() {
    for timer in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ignored-ack.db");
        let adapter = store(&path, clock());
        let commit = commit(timer);
        block_on(ProcessStore::commit(&adapter, &commit)).unwrap();
        let table = if timer {
            "process_timers"
        } else {
            "process_actions"
        };
        let column = if timer { "state" } else { "acknowledged" };
        let connection = Connection::open(&path).unwrap();
        if timer {
            block_on(adapter.schedule(&TimerSchedule {
                action: commit.actions[0].clone(),
                due_at: LogicalTime(100),
            }))
            .unwrap();
        }
        let outbox = if timer {
            None
        } else {
            Some(block_on(adapter.claim(&claim())).unwrap().remove(0))
        };
        let timer_lease = if timer {
            Some(
                block_on(adapter.claim_due(&timer_claim(100)))
                    .unwrap()
                    .remove(0),
            )
        } else {
            None
        };
        connection.execute_batch(&format!("CREATE TRIGGER ignore_ack BEFORE UPDATE OF {column} ON {table} BEGIN SELECT RAISE(IGNORE); END;")).unwrap();
        let acknowledge = || {
            timer_lease.as_ref().map_or_else(
                || {
                    block_on(OutboxStore::acknowledge(
                        &adapter,
                        outbox.as_ref().unwrap(),
                        LogicalTime(100),
                    ))
                },
                |lease| {
                    block_on(TimerClaimStore::acknowledge(
                        &adapter,
                        lease,
                        LogicalTime(100),
                    ))
                },
            )
        };
        assert_eq!(acknowledge(), Err(PortError::Invariant));
        let terminal: u32 = connection
            .query_row(&format!("SELECT {column} FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(terminal, 0);
        connection
            .execute_batch("DROP TRIGGER ignore_ack;")
            .unwrap();
        acknowledge().unwrap();
        // A valid repeated ACK remains idempotent after the first durable write.
        acknowledge().unwrap();
        let acknowledged: u32 = connection
            .query_row(&format!("SELECT {column} FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(acknowledged, 1);
    }
}

#[test]
fn exhausted_or_corrupt_lease_counters_and_clock_overflow_preserve_pending_work() {
    for timer_mode in [false, true] {
        for (next_token, now, expected) in [
            (0_i64, 100, PortError::Invariant),
            (-1, 100, PortError::Invariant),
            (i64::MAX, 100, PortError::QuotaExceeded),
            (1, u64::MAX, PortError::QuotaExceeded),
            (1, i64::MAX as u64, PortError::QuotaExceeded),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("leases.db");
            let clock = clock();
            let adapter = store(&path, clock.clone());
            let commit = commit(timer_mode);
            block_on(ProcessStore::commit(&adapter, &commit)).unwrap();
            if timer_mode {
                block_on(adapter.schedule(&TimerSchedule {
                    action: commit.actions[0].clone(),
                    due_at: LogicalTime(100),
                }))
                .unwrap();
            }
            let connection = Connection::open(&path).unwrap();
            // Deliberately damaged storage must fail closed even if an external
            // writer bypassed CHECK constraints; the adapter never does this.
            connection
                .execute_batch("PRAGMA ignore_check_constraints=ON")
                .unwrap();
            connection
                .execute("UPDATE lease_counter SET next_token=?1", [next_token])
                .unwrap();
            clock.now.store(now, Ordering::SeqCst);
            let error = if timer_mode {
                block_on(adapter.claim_due(&timer_claim(now))).unwrap_err()
            } else {
                block_on(adapter.claim(&claim())).unwrap_err()
            };
            assert_eq!(error, expected);
            let table = if timer_mode {
                "process_timers"
            } else {
                "process_actions"
            };
            let stored: (i64, bool, bool) = connection
                .query_row(
                    &format!("SELECT attempt,lease IS NULL,expires_at IS NULL FROM {table}"),
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
            assert_eq!(stored, (0, true, true));
            let counter: i64 = connection
                .query_row("SELECT next_token FROM lease_counter", [], |row| row.get(0))
                .unwrap();
            assert_eq!(counter, next_token);
            connection
                .execute("UPDATE lease_counter SET next_token=1", [])
                .unwrap();
            clock.now.store(100, Ordering::SeqCst);
            if timer_mode {
                assert_eq!(
                    block_on(adapter.claim_due(&timer_claim(100)))
                        .unwrap()
                        .len(),
                    1
                );
            } else {
                let leases = block_on(adapter.claim(&claim())).unwrap();
                assert_eq!(leases.len(), 1);
                assert_eq!(leases[0].record.delivery_attempt, 0);
            }
        }
    }
}

#[test]
fn ignored_token_counter_update_returns_no_lease_and_preserves_work() {
    for timer in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ignored-counter.db");
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
        connection.execute_batch(
            "CREATE TRIGGER ignore_claim BEFORE UPDATE OF next_token ON lease_counter BEGIN SELECT RAISE(IGNORE); END;"
        ).unwrap();
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
