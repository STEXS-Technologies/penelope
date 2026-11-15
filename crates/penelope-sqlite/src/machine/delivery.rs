//! Source-bound leased handoff over the journal's existing worker/connection.
use super::*;

/// Optional outbox view of an existing journal. The injected clock must be trusted
/// and monotonic across reopen; it is independent of captured event timestamps.
/// A lease coordinates idempotent receiver admission, never arbitrary execution.
pub struct SqliteMachineOutbox<
    A: MachineAuthorizer,
    C: MachineLeaseClock,
    V: MachineIntentAcceptor,
    const N: usize,
> {
    store: Arc<SqliteMachineStore<A, N>>,
    clock: Arc<C>,
    verifier: Arc<V>,
    consumer: MachineSourceId,
    limits: MachineOutboxLimits,
}
impl<A: MachineAuthorizer, C: MachineLeaseClock, V: MachineIntentAcceptor, const N: usize>
    SqliteMachineOutbox<A, C, V, N>
{
    /// # Errors
    /// Refuses zero or inconsistent claim, attempt, duration and storage bounds.
    pub fn new(
        store: Arc<SqliteMachineStore<A, N>>,
        clock: Arc<C>,
        verifier: Arc<V>,
        limits: MachineOutboxLimits,
    ) -> Result<Self, MachineStoreError> {
        if limits.lease_ticks == 0
            || limits.max_claims == 0
            || limits.max_attempts == 0
            || limits.max_claim_bytes < MachineIntentLease::METADATA_BYTES
            || limits.max_claim_bytes as u128 > u128::from(store.limits.max_scope_bytes)
        {
            return Err(MachineStoreError::Capacity);
        }
        let consumer = verifier.identity();
        Ok(Self {
            store,
            clock,
            verifier,
            consumer,
            limits,
        })
    }
}

mod state;
pub(super) use state::usage;
use state::*;

#[async_trait]
impl<A: MachineAuthorizer, C: MachineLeaseClock + 'static, V: MachineIntentAcceptor, const N: usize>
    MachineIntentOutbox for SqliteMachineOutbox<A, C, V, N>
{
    type Context = A::Context;
    type Receipt = V::Receipt;
    fn consumer(&self) -> MachineSourceId {
        self.consumer
    }
    async fn claim(
        &self,
        context: &Self::Context,
        request: MachineIntentClaim,
    ) -> Result<Vec<MachineIntentLease>, MachineStoreError> {
        self.store
            .authorizer
            .authorize(
                context,
                request.scope,
                MachineOperation::ClaimIntents {
                    owner: request.owner,
                },
            )
            .await?;
        let clock = Arc::clone(&self.clock);
        let limits = self.limits;
        let payloads = self.store.payloads;
        let storage = self.store.limits;
        self.store
            .worker
            .run(move |connection| {
                Ok(claim::<N>(
                    connection, request, &*clock, limits, payloads, storage,
                ))
            })
            .map_err(worker_error)?
            .await
            .map_err(worker_error)?
    }
    async fn validate(
        &self,
        context: &Self::Context,
        lease: &MachineIntentLease,
    ) -> Result<(), MachineStoreError> {
        let metadata = lease.metadata();
        self.store
            .authorizer
            .authorize(
                context,
                metadata.source.head.scope,
                MachineOperation::ValidateIntent {
                    consumer: self.consumer,
                    intent: metadata.source.intent.identity,
                    owner: metadata.owner,
                    token: metadata.token,
                },
            )
            .await?;
        let lease = copy_lease(lease, self.store.payloads)?;
        let clock = Arc::clone(&self.clock);
        let payloads = self.store.payloads;
        self.store
            .worker
            .run(move |connection| {
                let result = (|| {
                    let transaction = connection.transaction().map_err(|e| db_error(&e))?;
                    check::<N>(&transaction, &lease, payloads, &*clock, false).map(|_| ())
                })();
                Ok(result)
            })
            .map_err(worker_error)?
            .await
            .map_err(worker_error)?
    }
    async fn status(
        &self,
        context: &Self::Context,
        scope: MachineScope,
        intent: MachineIntentId,
    ) -> Result<Option<MachineIntentStatus>, MachineStoreError> {
        self.store
            .authorizer
            .authorize(context, scope, MachineOperation::ReadIntent { intent })
            .await?;
        let payloads = self.store.payloads;
        self.store
            .worker
            .run(move |connection| {
                let result = (|| {
                    let transaction = connection.transaction().map_err(|e| db_error(&e))?;
                    if read_head_in::<N>(&transaction, scope, payloads)?.is_none() {
                        return Ok(None);
                    }
                    usage(&transaction, scope)?;
                    let Some((source, _)) = source::<N>(&transaction, scope, intent, payloads)?
                    else {
                        return Ok(None);
                    };
                    let delivery = read_delivery(&transaction, scope, intent)?;
                    Ok(Some(MachineIntentStatus {
                        source,
                        attempt: delivery.map_or(MachineDeliveryAttempt::ZERO, |row| row.attempt),
                        accepted: delivery.and_then(|row| acceptance(source, row)),
                    }))
                })();
                Ok(result)
            })
            .map_err(worker_error)?
            .await
            .map_err(worker_error)?
    }
    async fn acknowledge(
        &self,
        context: &Self::Context,
        lease: &MachineIntentLease,
        receipt: &Self::Receipt,
    ) -> Result<MachineIntentAcceptance, MachineStoreError> {
        let metadata = lease.metadata();
        let consumer = self.consumer;
        self.store
            .authorizer
            .authorize(
                context,
                metadata.source.head.scope,
                MachineOperation::AcknowledgeIntent {
                    intent: metadata.source.intent.identity,
                    commit: metadata.source.commit,
                    owner: metadata.owner,
                    token: metadata.token,
                    consumer,
                },
            )
            .await?;
        let owned = copy_lease(lease, self.store.payloads)?;
        let clock = Arc::clone(&self.clock);
        let payloads = self.store.payloads;
        self.store
            .worker
            .run(move |connection| {
                let result = (|| {
                    let transaction = connection.transaction().map_err(|e| db_error(&e))?;
                    check::<N>(&transaction, &owned, payloads, &*clock, true).map(|_| ())
                })();
                Ok(result)
            })
            .map_err(worker_error)?
            .await
            .map_err(worker_error)??;
        let verified = self
            .verifier
            .verify(&metadata.source, lease.payload(), receipt)
            .await?;
        if verified.source != metadata.source || verified.consumer != consumer {
            return Err(MachineStoreError::Conflict);
        }
        let final_lease = copy_lease(lease, payloads)?;
        let final_clock = Arc::clone(&self.clock);
        self.store
            .worker
            .run(move |connection| {
                Ok(acknowledge::<N>(
                    connection,
                    &final_lease,
                    verified,
                    payloads,
                    &*final_clock,
                ))
            })
            .map_err(worker_error)?
            .await
            .map_err(worker_error)?
    }
}

fn claim<const N: usize>(
    connection: &mut Connection,
    request: MachineIntentClaim,
    clock: &impl MachineLeaseClock,
    limits: MachineOutboxLimits,
    payloads: MachinePayloadLimits,
    storage: SqliteLimits,
) -> Result<Vec<MachineIntentLease>, MachineStoreError> {
    if request.max_intents == 0
        || request.max_intents > limits.max_claims
        || request.max_bytes < MachineIntentLease::METADATA_BYTES
        || request.max_bytes > limits.max_claim_bytes
    {
        return Err(MachineStoreError::Capacity);
    }
    #[cfg(all(test, unix))]
    super::tests::cut("outbox-claim-before-transaction");
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| db_error(&e))?;
    if read_head_in::<N>(&transaction, request.scope, payloads)?.is_none() {
        return Ok(Vec::new());
    }
    let mut used = usage(&transaction, request.scope)?;
    let tick = now(&transaction, request.scope, clock)?;
    let expires = MachineLeaseTick::new(
        tick.get()
            .checked_add(limits.lease_ticks)
            .ok_or(MachineStoreError::Capacity)?,
    );
    let mut statement=transaction.prepare_cached("SELECT i.intent FROM machine_intents i JOIN machine_events e ON e.machine_key=i.machine_key AND e.event=i.event LEFT JOIN machine_intent_delivery d ON d.machine_key=i.machine_key AND d.intent=i.intent WHERE i.machine_key=?1 AND (d.intent IS NULL OR (d.receipt IS NULL AND d.expires<=?2)) ORDER BY e.revision,i.ordinal LIMIT ?3").map_err(|e|db_error(&e))?;
    let ids = statement
        .query_map(
            params![
                key(request.scope).as_slice(),
                tick.get().to_be_bytes().as_slice(),
                i64::from(request.max_intents)
            ],
            |row| Ok(column_bytes(row, 0, 32)?.to_vec()),
        )
        .map_err(|e| db_error(&e))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| db_error(&e))?;
    drop(statement);
    let mut leases = Vec::new();
    let mut bytes = 0_usize;
    for id in ids {
        let intent = MachineIntentId::from_bytes(fixed(&id)?);
        let (source, payload) = source::<N>(&transaction, request.scope, intent, payloads)?
            .ok_or(MachineStoreError::Corrupt)?;
        let next_bytes = bytes
            .checked_add(MachineIntentLease::METADATA_BYTES)
            .and_then(|v| v.checked_add(payload.len()))
            .filter(|v| *v <= request.max_bytes);
        let Some(next_bytes) = next_bytes else {
            if leases.is_empty() {
                return Err(MachineStoreError::Capacity);
            }
            break;
        };
        let previous = read_delivery(&transaction, request.scope, intent)?;
        let attempt = previous
            .map_or(Some(1), |row| row.attempt.get().checked_add(1))
            .filter(|n| *n <= u64::from(limits.max_attempts))
            .ok_or(MachineStoreError::Capacity)?;
        if previous.is_none() {
            used = used
                .checked_add(MachineIntentLease::METADATA_BYTES as u64)
                .ok_or(MachineStoreError::Capacity)?;
            let journal: i64 = transaction
                .query_row(
                    "SELECT bytes_used FROM machine_heads WHERE machine_key=?1",
                    [key(request.scope).as_slice()],
                    |row| row.get(0),
                )
                .map_err(|e| db_error(&e))?;
            if u64::try_from(journal)
                .map_err(|_invalid| MachineStoreError::Corrupt)?
                .checked_add(used)
                .is_none_or(|v| v > storage.max_scope_bytes)
            {
                return Err(MachineStoreError::Capacity);
            }
        }
        let token = MachineLeaseToken::new(
            crate::lease_counter::next(&transaction)
                .map_err(task_error)?
                .get(),
        );
        #[cfg(all(test, unix))]
        super::tests::cut("outbox-claim-after-token");
        let row = Delivery {
            owner: request.owner,
            token,
            expires,
            attempt: MachineDeliveryAttempt::new(attempt),
            accepted: None,
        };
        write(
            &transaction,
            "INSERT INTO machine_intent_delivery(machine_key,intent,owner,token,expires,attempt) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(machine_key,intent) DO UPDATE SET owner=excluded.owner,token=excluded.token,expires=excluded.expires,attempt=excluded.attempt",
            params![
                key(request.scope).as_slice(),
                intent.bytes().as_slice(),
                request.owner.bytes().as_slice(),
                i64::try_from(token.get()).map_err(|_invalid| MachineStoreError::Capacity)?,
                expires.get().to_be_bytes().as_slice(),
                i64::try_from(attempt).map_err(|_invalid| MachineStoreError::Capacity)?
            ],
        )?;
        #[cfg(all(test, unix))]
        super::tests::cut("outbox-claim-after-delivery");
        if read_delivery(&transaction, request.scope, intent)? != Some(row) {
            return Err(MachineStoreError::Corrupt);
        }
        leases.push(MachineIntentLease::decode(
            MachineIntentLeaseMetadata {
                source,
                owner: row.owner,
                token,
                expires,
                attempt: row.attempt,
            },
            &payload,
            payloads.max_intent_bytes,
        )?);
        bytes = next_bytes;
    }
    write(
        &transaction,
        "UPDATE machine_heads SET delivery_bytes=?2 WHERE machine_key=?1",
        params![
            key(request.scope).as_slice(),
            i64::try_from(used).map_err(|_invalid| MachineStoreError::Capacity)?
        ],
    )?;
    if usage(&transaction, request.scope)? != used {
        return Err(MachineStoreError::Corrupt);
    }
    record_time(&transaction, request.scope, tick)?;
    #[cfg(all(test, unix))]
    super::tests::cut("outbox-claim-after-usage");
    // Recheck every source and lease after all writes, including trigger effects.
    for lease in &leases {
        check::<N>(&transaction, lease, payloads, clock, false)?;
    }
    #[cfg(all(test, unix))]
    super::tests::cut("outbox-claim-after-validation");
    transaction.commit().map_err(|e| db_error(&e))?;
    #[cfg(all(test, unix))]
    super::tests::cut("outbox-claim-after-commit");
    Ok(leases)
}
fn acknowledge<const N: usize>(
    connection: &mut Connection,
    lease: &MachineIntentLease,
    verified: MachineIntentAcceptance,
    payloads: MachinePayloadLimits,
    clock: &impl MachineLeaseClock,
) -> Result<MachineIntentAcceptance, MachineStoreError> {
    #[cfg(all(test, unix))]
    super::tests::cut("outbox-ack-before-transaction");
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| db_error(&e))?;
    #[cfg(all(test, unix))]
    super::tests::cut("outbox-ack-after-begin");
    let (delivery, tick) = check::<N>(&transaction, lease, payloads, clock, true)?;
    let source = lease.metadata().source;
    if let Some(previous) = acceptance(source, delivery) {
        return if previous == verified {
            Ok(previous)
        } else {
            Err(MachineStoreError::Conflict)
        };
    }
    write(
        &transaction,
        "UPDATE machine_intent_delivery SET consumer=?3,receipt=?4 WHERE machine_key=?1 AND intent=?2 AND token=?5 AND receipt IS NULL",
        params![
            key(source.head.scope).as_slice(),
            source.intent.identity.bytes().as_slice(),
            verified.consumer.bytes().as_slice(),
            verified.receipt_digest.as_slice(),
            i64::try_from(delivery.token.get()).map_err(|_invalid| MachineStoreError::Corrupt)?
        ],
    )?;
    #[cfg(all(test, unix))]
    super::tests::cut("outbox-ack-after-write");
    record_time(&transaction, source.head.scope, tick)?;
    let (stored, _) = check::<N>(&transaction, lease, payloads, clock, true)?;
    if acceptance(source, stored) != Some(verified) {
        return Err(MachineStoreError::Corrupt);
    }
    #[cfg(all(test, unix))]
    super::tests::cut("outbox-ack-after-validation");
    transaction.commit().map_err(|e| db_error(&e))?;
    #[cfg(all(test, unix))]
    super::tests::cut("outbox-ack-after-commit");
    Ok(verified)
}
