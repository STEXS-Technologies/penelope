use super::*;

pub(super) fn check_scope(
    connection: &Connection,
    scope: MachineScope,
) -> Result<Option<()>, MachineStoreError> {
    let stored = connection
        .query_row(
            "SELECT scope FROM machine_heads WHERE machine_key=?1",
            [key(scope).as_slice()],
            |row| Ok(column_bytes(row, 0, 104)?.to_vec()),
        )
        .optional()
        .map_err(|e| db_error(&e))?;
    match stored {
        Some(bytes) if bytes == scope_bytes(scope) => Ok(Some(())),
        Some(bytes) if bytes.len() == 104 => Err(MachineStoreError::ScopeMismatch),
        Some(_) => Err(MachineStoreError::Corrupt),
        None => {
            let orphan: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM machine_events WHERE machine_key=?1) OR EXISTS(SELECT 1 FROM machine_intents WHERE machine_key=?1)", [key(scope).as_slice()], |row| row.get(0)).map_err(|e| db_error(&e))?;
            if orphan {
                Err(MachineStoreError::Corrupt)
            } else {
                Ok(None)
            }
        }
    }
}
fn fixed<const N: usize>(bytes: &[u8]) -> Result<[u8; N], MachineStoreError> {
    bytes
        .try_into()
        .map_err(|_invalid| MachineStoreError::Corrupt)
}
fn blob<'row>(
    row: &'row rusqlite::Row<'_>,
    index: usize,
    limit: usize,
) -> rusqlite::Result<&'row [u8]> {
    column_bytes(
        row,
        index,
        payload_limit(limit).map_err(|_invalid| rusqlite::Error::InvalidQuery)?,
    )
}

pub(super) fn read_commit<const N: usize>(
    connection: &Connection,
    scope: MachineScope,
    event: MachineEventId,
    limits: MachinePayloadLimits,
) -> Result<Option<MachineStoredCommit<N>>, MachineStoreError> {
    let stored_row = connection.query_row("SELECT record,event_payload,state,revision FROM machine_events WHERE machine_key=?1 AND event=?2",params![key(scope).as_slice(),event.bytes().as_slice()],|row| {
        let record = blob(row,0,limits.max_record_bytes)?;
        let input = blob(row,1,limits.max_event_bytes)?;
        let state = blob(row,2,limits.max_state_bytes)?;
        let bytes = record.len().checked_add(input.len()).and_then(|n| n.checked_add(state.len())).filter(|n| *n <= limits.max_batch_bytes).ok_or(rusqlite::Error::InvalidQuery)?;
        Ok((record.to_vec(),input.to_vec(),state.to_vec(),column_bytes(row,3,8)?.to_vec(),bytes))
    }).optional().map_err(|e| db_error(&e))?;
    let Some((wire, event_bytes, state, revision, mut total)) = stored_row else {
        return Ok(None);
    };
    let record =
        MachineCommitRecord::<N>::decode(&wire).map_err(|_invalid| MachineStoreError::Corrupt)?;
    let transition = record.header().transition;
    if transition.context.expected.scope != scope
        || transition.context.event != event
        || transition.revision.get().to_be_bytes() != fixed::<8>(&revision)?
    {
        return Err(MachineStoreError::Corrupt);
    }
    let mut statement = connection.prepare_cached("SELECT intent,ordinal,payload FROM machine_intents WHERE machine_key=?1 AND event=?2 ORDER BY ordinal").map_err(|e| db_error(&e))?;
    let mut rows = statement
        .query(params![key(scope).as_slice(), event.bytes().as_slice()])
        .map_err(|e| db_error(&e))?;
    let mut payloads = Vec::new();
    for (ordinal, metadata) in record.intents().enumerate() {
        let row = rows
            .next()
            .map_err(|e| db_error(&e))?
            .ok_or(MachineStoreError::Corrupt)?;
        if column_bytes(row, 0, 32).map_err(|e| db_error(&e))? != metadata.identity.bytes()
            || row.get::<_, i64>(1).map_err(|e| db_error(&e))? != integer(ordinal)?
        {
            return Err(MachineStoreError::Corrupt);
        }
        let bytes = blob(row, 2, limits.max_intent_bytes).map_err(|e| db_error(&e))?;
        total = total
            .checked_add(bytes.len())
            .filter(|n| *n <= limits.max_batch_bytes)
            .ok_or(MachineStoreError::Corrupt)?;
        if u64::try_from(bytes.len()).ok() != Some(metadata.payload_bytes)
            || Digest::of(bytes).0 != metadata.digest
        {
            return Err(MachineStoreError::Corrupt);
        }
        payloads.push(bytes.to_vec());
    }
    if rows.next().map_err(|e| db_error(&e))?.is_some() {
        return Err(MachineStoreError::Corrupt);
    }
    MachineStoredCommit::decode(
        &wire,
        &event_bytes,
        &state,
        payloads.iter().map(Vec::as_slice),
        limits,
    )
    .map(Some)
    .map_err(|_invalid| MachineStoreError::Corrupt)
}

pub(super) fn read_head_in<const N: usize>(
    connection: &Connection,
    scope: MachineScope,
    limits: MachinePayloadLimits,
) -> Result<Option<MachineSnapshot>, MachineStoreError> {
    if check_scope(connection, scope)?.is_none() {
        return Ok(None);
    }
    let (revision, state, digest, last, count, genesis_matches) = connection
        .query_row(
            "SELECT revision,state,state_digest,last_event,event_count,(state=genesis AND state_digest=genesis_digest) FROM machine_heads WHERE machine_key=?1",
            [key(scope).as_slice()],
            |row| {
                Ok((
                    column_bytes(row, 0, 8)?.to_vec(),
                    blob(row, 1, limits.max_state_bytes)?.to_vec(),
                    column_bytes(row, 2, 32)?.to_vec(),
                    if matches!(row.get_ref(3)?, rusqlite::types::ValueRef::Null) {
                        None
                    } else {
                        Some(column_bytes(row, 3, 32)?.to_vec())
                    },
                    row.get::<_,u32>(4)?,
                    row.get::<_,bool>(5)?,
                ))
            },
        )
        .map_err(|e| db_error(&e))?;
    let revision = MachineRevision::new(u64::from_be_bytes(fixed(&revision)?));
    if u64::from(count) != revision.get() || (revision == MachineRevision::ZERO && !genesis_matches)
    {
        return Err(MachineStoreError::Corrupt);
    }
    let head = MachineHead { scope, revision };
    let last_receipt = match last {
        Some(event) => {
            let stored = read_commit::<N>(
                connection,
                scope,
                MachineEventId::from_bytes(fixed(&event)?),
                limits,
            )?
            .ok_or(MachineStoreError::Corrupt)?;
            if stored.batch().state() != state || stored.receipt().head != head {
                return Err(MachineStoreError::Corrupt);
            }
            Some(stored.receipt())
        }
        None => None,
    };
    // The indexed tail must agree with the coherent head, including genesis.
    let tail: Option<Vec<u8>> = connection
        .query_row(
            "SELECT max(revision) FROM machine_events WHERE machine_key=?1",
            [key(scope).as_slice()],
            |row| row.get(0),
        )
        .map_err(|e| db_error(&e))?;
    let expected_tail = revision.get().to_be_bytes();
    if tail.as_deref()
        != (if revision == MachineRevision::ZERO {
            None
        } else {
            Some(expected_tail.as_slice())
        })
    {
        return Err(MachineStoreError::Corrupt);
    }
    MachineSnapshot::decode(head, &state, fixed(&digest)?, last_receipt, limits).map(Some)
}

pub(super) fn read_head<const N: usize>(
    connection: &mut Connection,
    scope: MachineScope,
    limits: MachinePayloadLimits,
) -> Result<Option<MachineSnapshot>, MachineStoreError> {
    let transaction = connection.transaction().map_err(|e| db_error(&e))?;
    read_head_in::<N>(&transaction, scope, limits)
}
pub(super) fn lookup<const N: usize>(
    connection: &mut Connection,
    scope: MachineScope,
    event: MachineEventId,
    limits: MachinePayloadLimits,
) -> Result<Option<MachineStoredCommit<N>>, MachineStoreError> {
    let transaction = connection.transaction().map_err(|e| db_error(&e))?;
    if read_head_in::<N>(&transaction, scope, limits)?.is_none() {
        return Ok(None);
    }
    read_commit(&transaction, scope, event, limits)
}
pub(super) fn history<const N: usize>(
    connection: &mut Connection,
    request: MachineHistoryRequest,
    limits: SqliteLimits,
    payloads: MachinePayloadLimits,
) -> Result<MachineHistoryPage<N>, MachineStoreError> {
    if request.max_records == 0
        || request.max_records > limits.max_history_records
        || request.max_bytes == 0
        || request.max_bytes as u128 > u128::from(limits.max_scope_bytes)
        || request.after > request.through
    {
        return Err(MachineStoreError::Capacity);
    }
    let transaction = connection.transaction().map_err(|e| db_error(&e))?;
    let head = read_head_in::<N>(&transaction, request.scope, payloads)?
        .ok_or(MachineStoreError::Conflict)?;
    if request.through > head.head().revision {
        return Err(MachineStoreError::StaleHead);
    }
    let mut statement = transaction.prepare_cached("SELECT event,revision FROM machine_events WHERE machine_key=?1 AND revision>?2 AND revision<=?3 ORDER BY revision LIMIT ?4").map_err(|e| db_error(&e))?;
    let mut rows = statement
        .query(params![
            key(request.scope).as_slice(),
            request.after.get().to_be_bytes().as_slice(),
            request.through.get().to_be_bytes().as_slice(),
            i64::from(request.max_records)
                .checked_add(1)
                .ok_or(MachineStoreError::Capacity)?
        ])
        .map_err(|e| db_error(&e))?;
    let mut records = Vec::new();
    let mut last = request.after;
    let mut bytes = 0_usize;
    while let Some(row) = rows.next().map_err(|e| db_error(&e))? {
        let event =
            MachineEventId::from_bytes(fixed(column_bytes(row, 0, 32).map_err(|e| db_error(&e))?)?);
        let revision = MachineRevision::new(u64::from_be_bytes(fixed(
            column_bytes(row, 1, 8).map_err(|e| db_error(&e))?,
        )?));
        if Some(revision) != last.checked_next() {
            return Err(MachineStoreError::Corrupt);
        }
        if records.len() == request.max_records as usize {
            return Ok(MachineHistoryPage {
                records,
                next: Some(last),
            });
        }
        let stored = read_commit(&transaction, request.scope, event, payloads)?
            .ok_or(MachineStoreError::Corrupt)?;
        let next_bytes = bytes
            .checked_add(stored.batch().byte_len())
            .filter(|n| *n <= request.max_bytes);
        let Some(next_bytes) = next_bytes else {
            if records.is_empty() {
                return Err(MachineStoreError::Capacity);
            }
            return Ok(MachineHistoryPage {
                records,
                next: Some(last),
            });
        };
        bytes = next_bytes;
        last = revision;
        records.push(stored);
    }
    if last != request.through {
        return Err(MachineStoreError::Corrupt);
    }
    Ok(MachineHistoryPage {
        records,
        next: None,
    })
}
