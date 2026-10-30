use crate::{
    SqliteProcessStore,
    process::{charge_process, db_error, decode, encode, integer, scope_process_key},
    tasks::copy_column,
};
use async_trait::async_trait;
use penelope_core::task::Digest;
use penelope_domain::{LogicalTime, ProcessAction};
use penelope_ports::*;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::num::NonZeroU64;

fn token(connection: &Connection) -> Result<OutboxLeaseToken, PortError> {
    let next: i64 = connection
        .query_row(
            "SELECT next_token FROM lease_counter WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| db_error(&error))?;
    let value = NonZeroU64::new(u64::try_from(next).map_err(|_negative| PortError::Invariant)?)
        .ok_or(PortError::Invariant)?;
    let following = next.checked_add(1).ok_or(PortError::QuotaExceeded)?;
    connection
        .execute(
            "UPDATE lease_counter SET next_token=?1 WHERE singleton=1 AND next_token=?2",
            params![following, next],
        )
        .map_err(|error| db_error(&error))?;
    Ok(OutboxLeaseToken::new(value))
}
fn expiry(now: LogicalTime, millis: u64) -> Result<LogicalTime, PortError> {
    let value = now.0.checked_add(millis).ok_or(PortError::QuotaExceeded)?;
    integer(value)?;
    Ok(LogicalTime(value))
}
fn outbox_lease(connection: &Connection, lease: &OutboxLease, limit: u32) -> Result<(), PortError> {
    let stored: Option<Vec<u8>> = connection
        .query_row(
            "SELECT lease FROM process_actions WHERE scope=?1 AND action_id=?2",
            params![
                lease.record.action.scope().canonical_bytes(),
                lease.record.action.action_id.as_str()
            ],
            |row| copy_column(row, 0, limit),
        )
        .optional()
        .map_err(|error| db_error(&error))?;
    if stored.as_deref() != Some(encode(lease, limit)?.as_slice()) {
        return Err(PortError::Conflict);
    }
    Ok(())
}
fn timer_lease(connection: &Connection, lease: &TimerLease, limit: u32) -> Result<(), PortError> {
    let stored: Option<Vec<u8>> = connection
        .query_row(
            "SELECT lease FROM process_timers WHERE scope=?1 AND action_id=?2 AND state IN (0,1)",
            params![
                lease.timer.action.scope().canonical_bytes(),
                lease.timer.action.action_id.as_str()
            ],
            |row| copy_column(row, 0, limit),
        )
        .optional()
        .map_err(|error| db_error(&error))?;
    if stored.as_deref() != Some(encode(lease, limit)?.as_slice()) {
        return Err(PortError::Conflict);
    }
    Ok(())
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
            let now = clock.now()?; let expires = expiry(now,millis)?;
            let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|error| db_error(&error))?;
            let key = request.scope.canonical_bytes();
            let pending = {
                let mut statement = transaction.prepare("SELECT action,checksum,attempt FROM process_actions WHERE scope=?1 AND acknowledged=0 AND (expires_at IS NULL OR expires_at<?2) ORDER BY action_id LIMIT ?3").map_err(|error| db_error(&error))?;
                let mut rows = statement.query(params![key,integer(now.0)?,request.limit.get()]).map_err(|error| db_error(&error))?;
                let mut pending = Vec::new();
                while let Some(row) = rows.next().map_err(|error| db_error(&error))? {
                    let bytes = copy_column(row,0,limits.max_record_bytes).map_err(|error| db_error(&error))?; let checksum = copy_column(row,1,32).map_err(|error| db_error(&error))?;
                    let action: ProcessAction = decode(&bytes,&checksum)?;
                    if action.scope() != request.scope || action.validate().is_err() { return Err(PortError::Invariant); }
                    let attempt: u32 = row.get(2).map_err(|error| db_error(&error))?;
                    if attempt >= MAX_OUTBOX_DELIVERY_ATTEMPTS { return Err(PortError::QuotaExceeded); }
                    pending.push((action,attempt));
                }
                pending
            };
            let mut leases = Vec::with_capacity(pending.len());
            for (action, attempt) in pending {
                let lease = OutboxLease { record: OutboxRecord { action,delivery_attempt: attempt,acknowledgement: OutboxAcknowledgement::Pending }, owner: request.owner.clone(), token: token(&transaction)?, lease_expires_at: expires };
                lease.validate_for_claim_at(&request,now)?;
                let encoded = encode(&lease,limits.max_record_bytes)?;
                transaction.execute("UPDATE process_actions SET attempt=?1,lease=?2,expires_at=?3 WHERE scope=?4 AND action_id=?5", params![attempt.checked_add(1).ok_or(PortError::QuotaExceeded)?,encoded,integer(expires.0)?,key,lease.record.action.action_id.as_str()]).map_err(|error| db_error(&error))?;
                leases.push(lease);
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
                transaction
                    .execute(
                        "UPDATE process_actions SET acknowledged=1 WHERE scope=?1 AND action_id=?2",
                        params![
                            lease.record.action.scope().canonical_bytes(),
                            lease.record.action.action_id.as_str()
                        ],
                    )
                    .map_err(|error| db_error(&error))?;
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
            let persisted = transaction.query_row("SELECT action,checksum FROM process_actions WHERE scope=?1 AND action_id=?2", params![key,timer.action.action_id.as_str()], |row| Ok((copy_column(row,0,limits.max_record_bytes)?,copy_column(row,1,32)?))).optional().map_err(|error| db_error(&error))?.ok_or(PortError::Conflict)?;
            if decode::<ProcessAction>(&persisted.0,&persisted.1)? != timer.action { return Err(PortError::Conflict); }
            let existing = transaction.query_row("SELECT timer,checksum FROM process_timers WHERE scope=?1 AND action_id=?2",params![key,timer.action.action_id.as_str()],|row| Ok((copy_column(row,0,limits.max_record_bytes)?,copy_column(row,1,32)?))).optional().map_err(|error| db_error(&error))?;
            if let Some((bytes,checksum)) = existing {
                if decode::<TimerSchedule>(&bytes,&checksum)? != timer { return Err(PortError::Conflict); }
                return Ok(());
            }
            let bytes = encode(&timer,limits.max_record_bytes)?;
            charge_process(&transaction, &scope_process_key(&timer.action.scope())?, u64::try_from(bytes.len()).map_err(|_overflow| PortError::QuotaExceeded)?.saturating_add(128), limits)?;
            transaction.execute("INSERT INTO process_timers(scope,action_id,timer,checksum,due_at) VALUES (?1,?2,?3,?4,?5)", params![key,timer.action.action_id.as_str(),bytes,Digest::of(&bytes).0.as_slice(),integer(timer.due_at.0)?]).map_err(|error| db_error(&error))?;
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
                let existing = transaction
                    .query_row(
                        "SELECT timer,checksum FROM process_timers WHERE scope=?1 AND action_id=?2",
                        params![key, timer.action.action_id.as_str()],
                        |row| {
                            Ok((
                                copy_column(row, 0, limits.max_record_bytes)?,
                                copy_column(row, 1, 32)?,
                            ))
                        },
                    )
                    .optional()
                    .map_err(|error| db_error(&error))?
                    .ok_or(PortError::Conflict)?;
                if decode::<TimerSchedule>(&existing.0, &existing.1)? != timer {
                    return Err(PortError::Conflict);
                }
                transaction
                    .execute(
                        "UPDATE process_timers SET state=2 WHERE scope=?1 AND action_id=?2",
                        params![key, timer.action.action_id.as_str()],
                    )
                    .map_err(|error| db_error(&error))?;
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
                let mut statement = transaction.prepare("SELECT timer,checksum,attempt FROM process_timers WHERE scope=?1 AND state=0 AND due_at<=?2 AND (expires_at IS NULL OR expires_at<?2) ORDER BY due_at,action_id LIMIT ?3").map_err(|error| db_error(&error))?;
                let mut rows = statement.query(params![key,integer(request.now.0)?,request.limit.get()]).map_err(|error| db_error(&error))?;
                let mut pending = Vec::new();
                while let Some(row) = rows.next().map_err(|error| db_error(&error))? {
                    let bytes = copy_column(row,0,limits.max_record_bytes).map_err(|error| db_error(&error))?; let checksum = copy_column(row,1,32).map_err(|error| db_error(&error))?;
                    let timer: TimerSchedule = decode(&bytes,&checksum)?;
                    let attempt: u32 = row.get(2).map_err(|error| db_error(&error))?;
                    if attempt >= MAX_OUTBOX_DELIVERY_ATTEMPTS { return Err(PortError::QuotaExceeded); }
                    pending.push((timer,attempt));
                }
                pending
            };
            let mut leases = Vec::with_capacity(pending.len());
            for (timer,attempt) in pending {
                let lease = TimerLease { timer, owner: request.owner.clone(), token: token(&transaction)?, lease_expires_at: expires };
                lease.validate_for_claim(&request)?;
                transaction.execute("UPDATE process_timers SET lease=?1,expires_at=?2,attempt=?3 WHERE scope=?4 AND action_id=?5",params![encode(&lease,limits.max_record_bytes)?,integer(expires.0)?,attempt.checked_add(1).ok_or(PortError::QuotaExceeded)?,key,lease.timer.action.action_id.as_str()]).map_err(|error| db_error(&error))?;
                leases.push(lease);
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
                transaction
                    .execute(
                        "UPDATE process_timers SET state=1 WHERE scope=?1 AND action_id=?2",
                        params![
                            lease.timer.action.scope().canonical_bytes(),
                            lease.timer.action.action_id.as_str()
                        ],
                    )
                    .map_err(|error| db_error(&error))?;
                transaction.commit().map_err(|error| db_error(&error))
            })?
            .await
    }
}
