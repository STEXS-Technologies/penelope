use crate::{
    SqliteProcessStore,
    process::{
        charge_process, db_error, decode, encode, encode_into, integer, port_error,
        scope_process_key,
    },
    schema::execute_one,
    tasks::column_bytes,
};
use async_trait::async_trait;
use penelope_core::task::Digest;
use penelope_domain::{LogicalTime, ProcessAction};
use penelope_ports::*;
use rusqlite::{Connection, TransactionBehavior, params};
use std::num::NonZeroU64;

fn token(connection: &Connection) -> Result<OutboxLeaseToken, PortError> {
    let next: i64 = connection
        .prepare_cached("SELECT next_token FROM lease_counter WHERE singleton=1")
        .map_err(|error| db_error(&error))?
        .query_row([], |row| row.get(0))
        .map_err(|error| db_error(&error))?;
    let value = NonZeroU64::new(u64::try_from(next).map_err(|_negative| PortError::Invariant)?)
        .ok_or(PortError::Invariant)?;
    let following = next.checked_add(1).ok_or(PortError::QuotaExceeded)?;
    let changed = connection
        .prepare_cached(
            "UPDATE lease_counter SET next_token=?1 WHERE singleton=1 AND next_token=?2",
        )
        .map_err(|error| db_error(&error))?
        .execute(params![following, next])
        .map_err(|error| db_error(&error))?;
    if changed != 1 {
        return Err(PortError::Invariant);
    }
    Ok(OutboxLeaseToken::new(value))
}
fn expiry(now: LogicalTime, millis: u64) -> Result<LogicalTime, PortError> {
    let value = now.0.checked_add(millis).ok_or(PortError::QuotaExceeded)?;
    integer(value)?;
    Ok(LogicalTime(value))
}
fn outbox_lease(connection: &Connection, lease: &OutboxLease, limit: u32) -> Result<(), PortError> {
    let mut statement = connection.prepare_cached(
        "SELECT lease,action,checksum,effect_key,expires_at,attempt FROM process_actions WHERE scope=?1 AND action_id=?2",
    ).map_err(|error| db_error(&error))?;
    let mut rows = statement
        .query(params![
            lease.record.action.scope().canonical_bytes(),
            lease.record.action.action_id.as_str()
        ])
        .map_err(|error| db_error(&error))?;
    let Some(row) = rows.next().map_err(|error| db_error(&error))? else {
        return Err(PortError::Conflict);
    };
    let stored = column_bytes(row, 0, limit).map_err(|error| db_error(&error))?;
    let bytes = column_bytes(row, 1, limit).map_err(|error| db_error(&error))?;
    let checksum = column_bytes(row, 2, 32).map_err(|error| db_error(&error))?;
    let key = column_bytes(row, 3, limit).map_err(|error| db_error(&error))?;
    let expires: i64 = row.get(4).map_err(|error| db_error(&error))?;
    let attempt: i64 = row.get(5).map_err(|error| db_error(&error))?;
    if stored != encode(lease, limit)? {
        return Err(PortError::Conflict);
    }
    // A matching lease alone cannot authorize mutation of a damaged action.
    if decode::<ProcessAction>(bytes, checksum)? != lease.record.action
        || key != lease.record.action.effect_key().canonical_bytes()
        || expires != integer(lease.lease_expires_at.0)?
        || attempt != integer(u64::from(lease.record.delivery_attempt).saturating_add(1))?
    {
        return Err(PortError::Invariant);
    }
    Ok(())
}
fn timer_lease(connection: &Connection, lease: &TimerLease, limit: u32) -> Result<(), PortError> {
    let mut statement = connection.prepare_cached(
        "SELECT lease,timer,checksum,due_at,expires_at,attempt FROM process_timers WHERE scope=?1 AND action_id=?2 AND state IN (0,1)",
    ).map_err(|error| db_error(&error))?;
    let mut rows = statement
        .query(params![
            lease.timer.action.scope().canonical_bytes(),
            lease.timer.action.action_id.as_str()
        ])
        .map_err(|error| db_error(&error))?;
    let Some(row) = rows.next().map_err(|error| db_error(&error))? else {
        return Err(PortError::Conflict);
    };
    let stored = column_bytes(row, 0, limit).map_err(|error| db_error(&error))?;
    let bytes = column_bytes(row, 1, limit).map_err(|error| db_error(&error))?;
    let checksum = column_bytes(row, 2, 32).map_err(|error| db_error(&error))?;
    let due: i64 = row.get(3).map_err(|error| db_error(&error))?;
    let expires: i64 = row.get(4).map_err(|error| db_error(&error))?;
    let attempt: i64 = row.get(5).map_err(|error| db_error(&error))?;
    if stored != encode(lease, limit)? {
        return Err(PortError::Conflict);
    }
    if decode::<TimerSchedule>(bytes, checksum)? != lease.timer
        || due != integer(lease.timer.due_at.0)?
        || expires != integer(lease.lease_expires_at.0)?
        || !(1..=i64::from(MAX_OUTBOX_DELIVERY_ATTEMPTS)).contains(&attempt)
    {
        return Err(PortError::Invariant);
    }
    Ok(())
}

// Decode while the SQLite row is alive; retain no row data or validation result
// between operations, including duplicate schedules and cancellation retries.
fn stored_timer(
    connection: &Connection,
    key: &[u8],
    action_id: &str,
    limit: u32,
) -> Result<Option<TimerSchedule>, PortError> {
    let mut statement = connection
        .prepare_cached("SELECT timer,checksum FROM process_timers WHERE scope=?1 AND action_id=?2")
        .map_err(|error| db_error(&error))?;
    let mut rows = statement
        .query(params![key, action_id])
        .map_err(|error| db_error(&error))?;
    let Some(row) = rows.next().map_err(|error| db_error(&error))? else {
        return Ok(None);
    };
    let bytes = column_bytes(row, 0, limit).map_err(|error| db_error(&error))?;
    let checksum = column_bytes(row, 1, 32).map_err(|error| db_error(&error))?;
    decode(bytes, checksum).map(Some)
}

#[async_trait]
impl OutboxStore for SqliteProcessStore {
    async fn claim(&self, request: &OutboxClaimRequest) -> Result<Vec<OutboxLease>, PortError> {
        request.validate()?;
        let request = request.clone();
        let limits = self.limits;
        let clock = self.clock.clone();
        let millis = self.lease_millis;
        self.worker.run(move |connection| {
            let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|error| db_error(&error))?;
            // Lock acquisition may wait beyond a lease window. Sample trusted
            // time only after this claim owns the writer transaction.
            let now = clock.now()?; let expires = expiry(now,millis)?;
            let key = request.scope.canonical_bytes();
            let pending = {
                let mut statement = transaction.prepare_cached("SELECT action,checksum,attempt,action_id,effect_key FROM process_actions WHERE scope=?1 AND acknowledged=0 AND (expires_at IS NULL OR expires_at<?2) ORDER BY action_id LIMIT ?3").map_err(|error| db_error(&error))?;
                let mut rows = statement.query(params![key,integer(now.0)?,request.limit.get()]).map_err(|error| db_error(&error))?;
                let mut pending = Vec::new();
                while let Some(row) = rows.next().map_err(|error| db_error(&error))? {
                    let bytes = column_bytes(row,0,limits.max_record_bytes).map_err(|error| db_error(&error))?; let checksum = column_bytes(row,1,32).map_err(|error| db_error(&error))?;
                    let action: ProcessAction = decode(bytes,checksum)?;
                    let stored_id = row.get_ref(3).map_err(|error| db_error(&error))?;
                    let stored_key = row.get_ref(4).map_err(|error| db_error(&error))?;
                    if action.scope() != request.scope || action.validate().is_err()
                        || stored_id.as_str().map_err(|_type| PortError::Invariant)? != action.action_id.as_str()
                        || stored_key.as_blob().map_err(|_type| PortError::Invariant)? != action.effect_key().canonical_bytes()
                    { return Err(PortError::Invariant); }
                    let attempt: u32 = row.get(2).map_err(|error| db_error(&error))?;
                    if attempt >= MAX_OUTBOX_DELIVERY_ATTEMPTS { return Err(PortError::QuotaExceeded); }
                    pending.push((action,attempt));
                }
                pending
            };
            let mut leases = Vec::with_capacity(pending.len());
            if !pending.is_empty() {
                let mut update = transaction.prepare_cached("UPDATE process_actions SET attempt=?1,lease=?2,expires_at=?3 WHERE scope=?4 AND action_id=?5").map_err(|error| db_error(&error))?;
                let mut encoded = Vec::with_capacity(128);
                for (action, attempt) in pending {
                    let lease = OutboxLease { record: OutboxRecord { action,delivery_attempt: attempt,acknowledgement: OutboxAcknowledgement::Pending }, owner: request.owner.clone(), token: token(&transaction)?, lease_expires_at: expires };
                    lease.validate_for_claim_at(&request,now)?;
                    encode_into(&lease,limits.max_record_bytes,&mut encoded)?;
                    let changed = update.execute(params![attempt.checked_add(1).ok_or(PortError::QuotaExceeded)?,encoded.as_slice(),integer(expires.0)?,key,lease.record.action.action_id.as_str()]).map_err(|error| db_error(&error))?;
                    if changed != 1 { return Err(PortError::Invariant); }
                    leases.push(lease);
                }
            }
            transaction.commit().map_err(|error| db_error(&error))?; Ok(leases)
        })?.await
    }
    async fn acknowledge(&self, lease: &OutboxLease, now: LogicalTime) -> Result<(), PortError> {
        lease.validate_at(now)?;
        let lease = lease.clone();
        let limits = self.limits;
        self.worker
            .run(move |connection| {
                let transaction = connection
                    .transaction_with_behavior(TransactionBehavior::Immediate)
                    .map_err(|error| db_error(&error))?;
                outbox_lease(&transaction, &lease, limits.max_record_bytes)?;
                let changed = transaction
                    .execute(
                        "UPDATE process_actions SET acknowledged=1 WHERE scope=?1 AND action_id=?2",
                        params![
                            lease.record.action.scope().canonical_bytes(),
                            lease.record.action.action_id.as_str()
                        ],
                    )
                    .map_err(|error| db_error(&error))?;
                if changed != 1 {
                    return Err(PortError::Invariant);
                }
                transaction.commit().map_err(|error| db_error(&error))
            })?
            .await
    }
    async fn renew(
        &self,
        lease: &OutboxLease,
        now: LogicalTime,
        new_expiry: LogicalTime,
    ) -> Result<OutboxLease, PortError> {
        let renewed = lease.renew_at(now, new_expiry)?;
        integer(new_expiry.0)?;
        let lease = lease.clone();
        let limits = self.limits;
        self.worker.run(move |connection| {
            let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|error| db_error(&error))?;
            outbox_lease(&transaction,&lease,limits.max_record_bytes)?;
            let changed = transaction.execute("UPDATE process_actions SET lease=?1,expires_at=?2 WHERE scope=?3 AND action_id=?4 AND acknowledged=0", params![encode(&renewed,limits.max_record_bytes)?,integer(new_expiry.0)?,lease.record.action.scope().canonical_bytes(),lease.record.action.action_id.as_str()]).map_err(|error| db_error(&error))?;
            if changed != 1 { return Err(PortError::Conflict); }
            transaction.commit().map_err(|error| db_error(&error))?; Ok(renewed)
        })?.await
    }
}

#[async_trait]
impl TimerScheduler for SqliteProcessStore {
    async fn schedule(&self, timer: &TimerSchedule) -> Result<(), PortError> {
        timer.validate().map_err(|_invalid| PortError::Invariant)?;
        integer(timer.due_at.0)?;
        let timer = timer.clone();
        let limits = self.limits;
        self.worker.run(move |connection| {
            let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|error| db_error(&error))?;
            let key = timer.action.scope().canonical_bytes();
            let persisted: ProcessAction = {
                let mut statement = transaction.prepare_cached("SELECT action,checksum FROM process_actions WHERE scope=?1 AND action_id=?2").map_err(|error| db_error(&error))?;
                let mut rows = statement.query(params![key,timer.action.action_id.as_str()]).map_err(|error| db_error(&error))?;
                let row = rows.next().map_err(|error| db_error(&error))?.ok_or(PortError::Conflict)?;
                let bytes = column_bytes(row,0,limits.max_record_bytes).map_err(|error| db_error(&error))?;
                let checksum = column_bytes(row,1,32).map_err(|error| db_error(&error))?;
                decode(bytes,checksum)?
            };
            if persisted != timer.action { return Err(PortError::Conflict); }
            if let Some(existing) = stored_timer(&transaction,&key,timer.action.action_id.as_str(),limits.max_record_bytes)? {
                if existing != timer { return Err(PortError::Conflict); }
                return Ok(());
            }
            let bytes = encode(&timer,limits.max_record_bytes)?;
            charge_process(&transaction, &scope_process_key(&timer.action.scope())?, u64::try_from(bytes.len()).map_err(|_overflow| PortError::QuotaExceeded)?.saturating_add(128), limits)?;
            execute_one(&transaction,"INSERT INTO process_timers(scope,action_id,timer,checksum,due_at) VALUES (?1,?2,?3,?4,?5)", params![key,timer.action.action_id.as_str(),bytes,Digest::of(&bytes).0.as_slice(),integer(timer.due_at.0)?]).map_err(port_error)?;
            transaction.commit().map_err(|error| db_error(&error))
        })?.await
    }
    async fn cancel(&self, timer: &TimerSchedule) -> Result<(), PortError> {
        timer.validate().map_err(|_invalid| PortError::Invariant)?;
        let timer = timer.clone();
        let limits = self.limits;
        self.worker
            .run(move |connection| {
                let transaction = connection
                    .transaction_with_behavior(TransactionBehavior::Immediate)
                    .map_err(|error| db_error(&error))?;
                let key = timer.action.scope().canonical_bytes();
                let existing = stored_timer(
                    &transaction,
                    &key,
                    timer.action.action_id.as_str(),
                    limits.max_record_bytes,
                )?
                .ok_or(PortError::Conflict)?;
                if existing != timer {
                    return Err(PortError::Conflict);
                }
                execute_one(
                    &transaction,
                    "UPDATE process_timers SET state=2 WHERE scope=?1 AND action_id=?2",
                    params![key, timer.action.action_id.as_str()],
                )
                .map_err(port_error)?;
                transaction.commit().map_err(|error| db_error(&error))
            })?
            .await
    }
}

#[async_trait]
impl TimerClaimStore for SqliteProcessStore {
    async fn claim_due(&self, request: &TimerClaimRequest) -> Result<Vec<TimerLease>, PortError> {
        request.validate()?;
        let request = request.clone();
        let limits = self.limits;
        let millis = self.lease_millis;
        self.worker.run(move |connection| {
            let expires = expiry(request.now,millis)?;
            let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|error| db_error(&error))?;
            let key = request.scope.canonical_bytes();
            let pending = {
                let mut statement = transaction.prepare_cached("SELECT timer,checksum,attempt,action_id,due_at FROM process_timers WHERE scope=?1 AND state=0 AND due_at<=?2 AND (expires_at IS NULL OR expires_at<?2) ORDER BY due_at,action_id LIMIT ?3").map_err(|error| db_error(&error))?;
                let mut rows = statement.query(params![key,integer(request.now.0)?,request.limit.get()]).map_err(|error| db_error(&error))?;
                let mut pending = Vec::new();
                while let Some(row) = rows.next().map_err(|error| db_error(&error))? {
                    let bytes = column_bytes(row,0,limits.max_record_bytes).map_err(|error| db_error(&error))?; let checksum = column_bytes(row,1,32).map_err(|error| db_error(&error))?;
                    let timer: TimerSchedule = decode(bytes,checksum)?;
                    let stored_id = row.get_ref(3).map_err(|error| db_error(&error))?;
                    let due: i64 = row.get(4).map_err(|error| db_error(&error))?;
                    if timer.action.scope() != request.scope || timer.validate().is_err()
                        || stored_id.as_str().map_err(|_type| PortError::Invariant)? != timer.action.action_id.as_str()
                        || due != integer(timer.due_at.0)?
                    { return Err(PortError::Invariant); }
                    let attempt: u32 = row.get(2).map_err(|error| db_error(&error))?;
                    if attempt >= MAX_OUTBOX_DELIVERY_ATTEMPTS { return Err(PortError::QuotaExceeded); }
                    pending.push((timer,attempt));
                }
                pending
            };
            let mut leases = Vec::with_capacity(pending.len());
            if !pending.is_empty() {
                let mut update = transaction.prepare_cached("UPDATE process_timers SET lease=?1,expires_at=?2,attempt=?3 WHERE scope=?4 AND action_id=?5").map_err(|error| db_error(&error))?;
                let mut encoded = Vec::with_capacity(128);
                for (timer,attempt) in pending {
                    let lease = TimerLease { timer, owner: request.owner.clone(), token: token(&transaction)?, lease_expires_at: expires };
                    lease.validate_for_claim(&request)?;
                    encode_into(&lease,limits.max_record_bytes,&mut encoded)?;
                    let changed = update.execute(params![encoded.as_slice(),integer(expires.0)?,attempt.checked_add(1).ok_or(PortError::QuotaExceeded)?,key,lease.timer.action.action_id.as_str()]).map_err(|error| db_error(&error))?;
                    if changed != 1 { return Err(PortError::Invariant); }
                    leases.push(lease);
                }
            }
            transaction.commit().map_err(|error| db_error(&error))?; Ok(leases)
        })?.await
    }
    async fn acknowledge(&self, lease: &TimerLease, now: LogicalTime) -> Result<(), PortError> {
        lease.validate_at(now)?;
        let lease = lease.clone();
        let limits = self.limits;
        self.worker
            .run(move |connection| {
                let transaction = connection
                    .transaction_with_behavior(TransactionBehavior::Immediate)
                    .map_err(|error| db_error(&error))?;
                timer_lease(&transaction, &lease, limits.max_record_bytes)?;
                let changed = transaction
                    .execute(
                        "UPDATE process_timers SET state=1 WHERE scope=?1 AND action_id=?2",
                        params![
                            lease.timer.action.scope().canonical_bytes(),
                            lease.timer.action.action_id.as_str()
                        ],
                    )
                    .map_err(|error| db_error(&error))?;
                if changed != 1 {
                    return Err(PortError::Invariant);
                }
                transaction.commit().map_err(|error| db_error(&error))
            })?
            .await
    }
}
