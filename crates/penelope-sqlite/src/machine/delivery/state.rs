//! Bounded immutable source reconstruction and exact lease validation.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Delivery {
    pub(super) owner: MachineLeaseOwner,
    pub(super) token: MachineLeaseToken,
    pub(super) expires: MachineLeaseTick,
    pub(super) attempt: MachineDeliveryAttempt,
    pub(super) accepted: Option<(MachineSourceId, [u8; 32])>,
}
fn sql_fixed<const BYTES: usize>(
    row: &rusqlite::Row<'_>,
    column: usize,
) -> rusqlite::Result<[u8; BYTES]> {
    column_bytes(
        row,
        column,
        u32::try_from(BYTES).map_err(|_invalid| rusqlite::Error::InvalidQuery)?,
    )?
    .try_into()
    .map_err(|_invalid| rusqlite::Error::InvalidQuery)
}
fn delivery_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Delivery> {
    let token: i64 = row.get(1)?;
    let expires = u64::from_be_bytes(sql_fixed(row, 2)?);
    let attempt: u16 = row.get(3)?;
    if token <= 0 || attempt == 0 || expires == 0 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let accepted = match (row.get_ref(4)?, row.get_ref(5)?) {
        (rusqlite::types::ValueRef::Null, rusqlite::types::ValueRef::Null) => None,
        (rusqlite::types::ValueRef::Blob(_), rusqlite::types::ValueRef::Blob(_)) => Some((
            MachineSourceId::from_bytes(sql_fixed(row, 4)?),
            sql_fixed(row, 5)?,
        )),
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    Ok(Delivery {
        owner: MachineLeaseOwner::from_bytes(sql_fixed(row, 0)?),
        token: MachineLeaseToken::new(
            u64::try_from(token).map_err(|_invalid| rusqlite::Error::InvalidQuery)?,
        ),
        expires: MachineLeaseTick::new(expires),
        attempt: MachineDeliveryAttempt::new(u64::from(attempt)),
        accepted,
    })
}
pub(super) fn read_delivery(
    connection: &Connection,
    scope: MachineScope,
    intent: MachineIntentId,
) -> Result<Option<Delivery>, MachineStoreError> {
    connection.query_row(
        "SELECT owner,token,expires,attempt,consumer,receipt FROM machine_intent_delivery WHERE machine_key=?1 AND intent=?2",
        params![key(scope).as_slice(), intent.bytes().as_slice()], delivery_row,
    ).optional().map_err(|e| db_error(&e))
}
pub(super) fn source<const N: usize>(
    connection: &Connection,
    scope: MachineScope,
    intent: MachineIntentId,
    limits: MachinePayloadLimits,
) -> Result<Option<(MachineIntentSource, Vec<u8>)>, MachineStoreError> {
    let event = connection
        .query_row(
            "SELECT event FROM machine_intents WHERE machine_key=?1 AND intent=?2",
            params![key(scope).as_slice(), intent.bytes().as_slice()],
            |row| Ok(column_bytes(row, 0, 32)?.to_vec()),
        )
        .optional()
        .map_err(|e| db_error(&e))?;
    let Some(event) = event else {
        if read_delivery(connection, scope, intent)?.is_some() {
            return Err(MachineStoreError::Corrupt);
        }
        return Ok(None);
    };
    let event = MachineEventId::from_bytes(fixed(&event)?);
    let commit =
        read_commit::<N>(connection, scope, event, limits)?.ok_or(MachineStoreError::Corrupt)?;
    let (metadata, payload) = commit
        .batch()
        .intents()
        .find(|(metadata, _)| metadata.identity == intent)
        .ok_or(MachineStoreError::Corrupt)?;
    Ok(Some((
        MachineIntentSource {
            head: commit.receipt().head,
            event,
            commit: commit.receipt().identity,
            intent: *metadata,
        },
        payload.to_vec(),
    )))
}
/// Delivery metadata is charged once, independent of reclaim/ack count. Journal
/// bytes retain their original accounting; all writers enforce their sum.
pub(crate) fn usage(
    connection: &Connection,
    scope: MachineScope,
) -> Result<u64, MachineStoreError> {
    let (bytes,count):(i64,i64) = connection.query_row("SELECT delivery_bytes,(SELECT count(*) FROM machine_intent_delivery WHERE machine_key=?1) FROM machine_heads WHERE machine_key=?1",[key(scope).as_slice()],|row|Ok((row.get(0)?,row.get(1)?))).map_err(|e|db_error(&e))?;
    let bytes = u64::try_from(bytes).map_err(|_invalid| MachineStoreError::Corrupt)?;
    let count = u64::try_from(count).map_err(|_invalid| MachineStoreError::Corrupt)?;
    if count.checked_mul(MachineIntentLease::METADATA_BYTES as u64) != Some(bytes) {
        return Err(MachineStoreError::Corrupt);
    }
    let damaged: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM machine_intent_delivery d LEFT JOIN machine_intents i ON i.machine_key=d.machine_key AND i.intent=d.intent WHERE d.machine_key=?1 AND (i.intent IS NULL OR typeof(d.owner)!='blob' OR length(d.owner)!=32 OR typeof(d.token)!='integer' OR d.token<=0 OR d.token>=(SELECT next_token FROM lease_counter WHERE singleton=1) OR typeof(d.expires)!='blob' OR length(d.expires)!=8 OR d.expires=x'0000000000000000' OR typeof(d.attempt)!='integer' OR d.attempt NOT BETWEEN 1 AND 65535 OR (d.consumer IS NULL)!=(d.receipt IS NULL) OR (d.consumer IS NOT NULL AND (typeof(d.consumer)!='blob' OR length(d.consumer)!=32 OR typeof(d.receipt)!='blob' OR length(d.receipt)!=32)))) OR EXISTS(SELECT 1 FROM machine_intents i LEFT JOIN machine_events e ON e.machine_key=i.machine_key AND e.event=i.event WHERE i.machine_key=?1 AND e.event IS NULL)",
        [key(scope).as_slice()], |row| row.get(0),
    ).map_err(|e| db_error(&e))?;
    if damaged {
        return Err(MachineStoreError::Corrupt);
    }
    Ok(bytes)
}
pub(super) fn now(
    connection: &Connection,
    scope: MachineScope,
    clock: &impl MachineLeaseClock,
) -> Result<MachineLeaseTick, MachineStoreError> {
    let previous = connection
        .query_row(
            "SELECT lease_tick FROM machine_heads WHERE machine_key=?1",
            [key(scope).as_slice()],
            |row| Ok(column_bytes(row, 0, 8)?.to_vec()),
        )
        .map_err(|e| db_error(&e))?;
    let now = clock.now()?;
    if now.get() < u64::from_be_bytes(fixed(&previous)?) {
        return Err(MachineStoreError::Corrupt);
    }
    Ok(now)
}
pub(super) fn record_time(
    connection: &Connection,
    scope: MachineScope,
    tick: MachineLeaseTick,
) -> Result<(), MachineStoreError> {
    write(
        connection,
        "UPDATE machine_heads SET lease_tick=?2 WHERE machine_key=?1",
        params![key(scope).as_slice(), tick.get().to_be_bytes().as_slice()],
    )?;
    let stored = connection
        .query_row(
            "SELECT lease_tick FROM machine_heads WHERE machine_key=?1",
            [key(scope).as_slice()],
            |row| Ok(column_bytes(row, 0, 8)?.to_vec()),
        )
        .map_err(|e| db_error(&e))?;
    if stored != tick.get().to_be_bytes() {
        return Err(MachineStoreError::Corrupt);
    }
    Ok(())
}
pub(super) fn acceptance(
    source: MachineIntentSource,
    delivery: Delivery,
) -> Option<MachineIntentAcceptance> {
    delivery
        .accepted
        .map(|(consumer, receipt_digest)| MachineIntentAcceptance {
            source,
            consumer,
            receipt_digest,
        })
}
pub(super) fn check<const N: usize>(
    connection: &Connection,
    lease: &MachineIntentLease,
    payloads: MachinePayloadLimits,
    clock: &impl MachineLeaseClock,
    allow_accepted: bool,
) -> Result<(Delivery, MachineLeaseTick), MachineStoreError> {
    let metadata = lease.metadata();
    let scope = metadata.source.head.scope;
    read_head_in::<N>(connection, scope, payloads)?.ok_or(MachineStoreError::Conflict)?;
    usage(connection, scope)?;
    let (original, payload) =
        source::<N>(connection, scope, metadata.source.intent.identity, payloads)?
            .ok_or(MachineStoreError::Conflict)?;
    let delivery = read_delivery(connection, scope, original.intent.identity)?
        .ok_or(MachineStoreError::Conflict)?;
    if original != metadata.source
        || payload != lease.payload()
        || delivery.owner != metadata.owner
        || delivery.token != metadata.token
        || delivery.expires != metadata.expires
        || delivery.attempt != metadata.attempt
    {
        return Err(MachineStoreError::Conflict);
    }
    let tick = now(connection, scope, clock)?;
    if delivery.accepted.is_some() {
        if !allow_accepted {
            return Err(MachineStoreError::Conflict);
        }
    } else if delivery.expires <= tick {
        return Err(MachineStoreError::Conflict);
    }
    Ok((delivery, tick))
}
pub(super) fn copy_lease(
    lease: &MachineIntentLease,
    limits: MachinePayloadLimits,
) -> Result<MachineIntentLease, MachineStoreError> {
    MachineIntentLease::decode(lease.metadata(), lease.payload(), limits.max_intent_bytes)
}
