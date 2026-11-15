//! Source-authorized saga bootstrap through the existing engines and process ports.
use crate::{engine::*, graph::*};
use async_trait::async_trait;
use penelope_core::machine::MachineSourceId;
use penelope_domain::*;
use penelope_ports::{machine::*, payloads::ProcessPayloadStore, *};
use serde::{Deserialize, Serialize};
use std::{fmt::Write, num::NonZeroU16, sync::Arc};

/// Exact host-pinned workflow policy. Delivery starts one instance per source;
/// it never applies results, retries, compensation or external effects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MachineSagaDefinition {
    Linear(LinearSagaDefinition),
    Graph(ProcessGraphDefinition),
}
#[derive(Debug, thiserror::Error)]
pub enum MachineSagaHandoffError {
    #[error(transparent)]
    Source(#[from] MachineStoreError),
    #[error(transparent)]
    Port(#[from] PortError),
    #[error(transparent)]
    Linear(#[from] EngineError),
    #[error(transparent)]
    Graph(#[from] GraphEngineError),
    #[error(transparent)]
    Identifier(#[from] DomainError),
}
/// Native bootstrap batch recovered from authoritative outcomes and payloads.
/// Private construction is convenience, not authentication: ACK verifies storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineSagaReceipt {
    source: MachineIntentSource,
    consumer: MachineSourceId,
    commit: AtomicProcessCommit,
}
impl MachineSagaReceipt {
    pub const fn source(&self) -> MachineIntentSource {
        self.source
    }
    pub const fn consumer(&self) -> MachineSourceId {
        self.consumer
    }
    pub const fn commit(&self) -> &AtomicProcessCommit {
        &self.commit
    }
}
/// Trusted receiver policy over native process/payload stores and authorization.
/// The principal is a host-authenticated service identity, never a payload field.
pub struct MachineSagaReceiver<S, A> {
    store: Arc<S>,
    authorizer: Arc<A>,
    principal: PrincipalId,
    consumer: MachineSourceId,
    definition: MachineSagaDefinition,
    max_input_bytes: u32,
}
fn identifier(prefix: &str, digest: [u8; 32]) -> Result<String, PortError> {
    let mut text = String::from(prefix);
    for byte in digest {
        write!(&mut text, "{byte:02x}").map_err(|_format| PortError::Invariant)?;
    }
    Ok(text)
}
impl<S: ProcessStore + ProcessPayloadStore + ProcessCommitVerifier, A: ProcessAuthorizer>
    MachineSagaReceiver<S, A>
{
    /// # Errors
    /// Rejects invalid workflow policy or an unencodable receiver identity.
    pub fn new(
        store: Arc<S>,
        authorizer: Arc<A>,
        namespace: MachineSourceId,
        principal: PrincipalId,
        definition: MachineSagaDefinition,
        max_input_bytes: u32,
    ) -> Result<Self, MachineSagaHandoffError> {
        match &definition {
            MachineSagaDefinition::Linear(value) => value.validate()?,
            MachineSagaDefinition::Graph(value) => value
                .validate()
                .map_err(|source| GraphEngineError::InvalidDefinition { source })?,
        }
        let mut identity = b"penelope.machine-saga-receiver.v1\0".to_vec();
        identity.extend_from_slice(&namespace.bytes());
        identity.extend_from_slice(&max_input_bytes.to_le_bytes());
        serde_json::to_writer(&mut identity, &(&principal, &definition))
            .map_err(|_codec| PortError::Invariant)?;
        Ok(Self {
            store,
            authorizer,
            principal,
            consumer: MachineSourceId::from_bytes(ContentDigest::sha256(&identity).0),
            definition,
            max_input_bytes,
        })
    }
    /// Structural identity only; this does not authenticate a source or start work.
    /// # Errors
    /// Rejects an invalid derived native identifier.
    pub fn scope(
        &self,
        source: MachineIntentSource,
    ) -> Result<ProcessScope, MachineSagaHandoffError> {
        let mut tenant = b"penelope.machine-saga-tenant.v1\0".to_vec();
        tenant.extend_from_slice(&source.head.scope.tenant.bytes());
        let tenant = identifier("tnt_", ContentDigest::sha256(&tenant).0)?.parse()?;
        let process = identifier("prc_", source.binding_identity(self.consumer))?.parse()?;
        let (id, version, digest) = match &self.definition {
            MachineSagaDefinition::Linear(d) => {
                (&d.definition_id, &d.definition_version, d.definition_digest)
            }
            MachineSagaDefinition::Graph(d) => {
                (&d.definition_id, &d.definition_version, d.definition_digest)
            }
        };
        Ok(ProcessScope::new(
            tenant,
            process,
            id.clone(),
            version.clone(),
            digest,
        ))
    }
    fn validate_source(
        &self,
        source: MachineIntentSource,
        payload: &[u8],
    ) -> Result<(), MachineSagaHandoffError> {
        let len = u32::try_from(payload.len()).map_err(|_size| PortError::QuotaExceeded)?;
        if len > self.max_input_bytes {
            return Err(PortError::QuotaExceeded.into());
        }
        if source.head.revision == penelope_core::machine::MachineRevision::ZERO
            || source.intent.payload_bytes != u64::from(len)
            || source.intent.digest != ContentDigest::sha256(payload).0
        {
            return Err(PortError::Invariant.into());
        }
        Ok(())
    }
    async fn authorize(&self, scope: &ProcessScope) -> Result<(), MachineSagaHandoffError> {
        self.authorizer
            .authorize(&ProcessAuthorizationRequest {
                scope: scope.clone(),
                principal_id: self.principal.clone(),
                operation: ProcessAuthorizationOperation::Start,
            })
            .await?
            .require_authorized()?;
        Ok(())
    }
    fn plan(
        &self,
        source: MachineIntentSource,
        time: LogicalTime,
    ) -> Result<(AtomicProcessCommit, Vec<u8>), MachineSagaHandoffError> {
        let scope = self.scope(source)?;
        let identity = source.binding_identity(self.consumer);
        let input: InputId = identifier("inp_", identity)?.parse()?;
        let action: ActionId = identifier("act_", identity)?.parse()?;
        let make_facts = |kinds: Vec<ProcessOutcomeKind>, digest: ContentDigest| {
            kinds
                .into_iter()
                .enumerate()
                .map(|(index, kind)| {
                    let id = format!("{}_{}", identifier("out_", identity)?, index).parse()?;
                    Ok(ProcessOutcomeFact::new(
                        id,
                        CausationId::Input(input.clone()),
                        OutcomeActor::Principal(self.principal.clone()),
                        time,
                        kind,
                        digest,
                    ))
                })
                .collect::<Result<Vec<_>, MachineSagaHandoffError>>()
        };
        let (outcomes, action, bytes) = match &self.definition {
            MachineSagaDefinition::Linear(definition) => {
                let decision = start(
                    definition,
                    scope.tenant_id.clone(),
                    scope.process_id.clone(),
                    action.clone(),
                )?;
                let event = LinearSagaEvent::started(definition, input.clone(), action);
                let bytes = serde_json::to_vec(&event).map_err(|_codec| PortError::Invariant)?;
                let kinds = event
                    .observed_outcome_kinds()
                    .iter()
                    .chain(decision.planned_outcome_kinds())
                    .copied()
                    .collect();
                let facts = make_facts(kinds, ContentDigest::sha256(&bytes))?;
                (
                    decision.build_required_outcomes(&event, &scope, 0, &facts)?,
                    decision.next_action,
                    bytes,
                )
            }
            MachineSagaDefinition::Graph(definition) => {
                let decision = start_graph(
                    definition,
                    scope.tenant_id.clone(),
                    scope.process_id.clone(),
                    GraphSagaInput::Start {
                        input_id: input.clone(),
                        action_id: action,
                    },
                )?;
                let bytes =
                    serde_json::to_vec(&decision.event).map_err(|_codec| PortError::Invariant)?;
                let kinds = decision
                    .event
                    .observed_outcome_kinds()
                    .iter()
                    .chain(decision.planned_outcome_kinds())
                    .copied()
                    .collect();
                let facts = make_facts(kinds, ContentDigest::sha256(&bytes))?;
                (
                    decision.build_required_outcomes(&scope, 0, &facts)?,
                    decision.next_action,
                    bytes,
                )
            }
        };
        // This is process bootstrap, not a fabricated canonical/timer/manual input.
        let commit = AtomicProcessCommit::new(0, None, outcomes, action.into_iter().collect())
            .map_err(|_commit| PortError::Invariant)?;
        Ok((commit, bytes))
    }
    async fn initial(
        &self,
        scope: &ProcessScope,
    ) -> Result<Vec<ProcessOutcome>, MachineSagaHandoffError> {
        // Both native start plans require InputAccepted, Started, ActionPlanned.
        let request = OutcomeReplayRequest::new(
            scope.clone(),
            0,
            NonZeroU16::new(3).ok_or(PortError::Invariant)?,
        )
        .map_err(|_request| PortError::Invariant)?;
        let page = self.store.read_outcomes(&request).await?;
        page.validate().map_err(|_page| PortError::Invariant)?;
        if page.scope != *scope || page.from_sequence != 0 || page.outcomes.len() > 3 {
            return Err(PortError::Invariant.into());
        }
        Ok(page.outcomes)
    }
    async fn verify_native(
        &self,
        source: MachineIntentSource,
        payload: &[u8],
        receipt: &MachineSagaReceipt,
    ) -> Result<(), MachineSagaHandoffError> {
        if receipt.source != source || receipt.consumer != self.consumer {
            return Err(PortError::Conflict.into());
        }
        let time = receipt
            .commit
            .outcomes
            .first()
            .ok_or(PortError::Invariant)?
            .occurred_at;
        let (expected, event) = self.plan(source, time)?;
        if receipt.commit != expected {
            return Err(PortError::Invariant.into());
        }
        self.store
            .verify_commit(&expected)
            .await?
            .validate_for(&expected)
            .map_err(|_receipt| PortError::Invariant)?;
        let scope = self.scope(source)?;
        for (digest, bytes) in [
            (ContentDigest::sha256(&event), event.as_slice()),
            (ContentDigest(source.intent.digest), payload),
        ] {
            let stored = self.store.read_payload(&scope, digest).await?;
            if stored != bytes || ContentDigest::sha256(&stored) != digest {
                return Err(PortError::Invariant.into());
            }
        }
        Ok(())
    }
    /// Source-authorized, native-policy-authorized durable bootstrap. The returned
    /// receipt proves admission only. Dispatch remains in the native saga outbox.
    /// A failed or dropped response may leave a committed saga: reclaim and read
    /// the same identity; never start a replacement process to guess recovery.
    /// # Errors
    /// Refuses invalid policy/source, denied start, native storage or receipt mismatch.
    pub async fn accept<O: MachineIntentOutbox<Receipt = MachineSagaReceipt>>(
        &self,
        outbox: &O,
        context: &O::Context,
        lease: &MachineIntentLease,
        time: LogicalTime,
    ) -> Result<MachineSagaReceipt, MachineSagaHandoffError> {
        if outbox.consumer() != self.consumer {
            return Err(MachineStoreError::Conflict.into());
        }
        outbox.validate(context, lease).await?;
        let source = lease.metadata().source;
        self.validate_source(source, lease.payload())?;
        let scope = self.scope(source)?;
        self.authorize(&scope).await?;
        let existing = self.initial(&scope).await?;
        let time = existing.first().map_or(time, |record| record.occurred_at);
        let (commit, event) = self.plan(source, time)?;
        let receipt = MachineSagaReceipt {
            source,
            consumer: self.consumer,
            commit,
        };
        if existing.is_empty() {
            for bytes in [lease.payload(), event.as_slice()] {
                let digest = ContentDigest::sha256(bytes);
                if self.store.put_payload(&scope, digest, bytes).await? != digest {
                    return Err(PortError::Invariant.into());
                }
            }
            match self.store.commit(&receipt.commit).await {
                Ok(ack) => ack
                    .validate_for(&receipt.commit)
                    .map_err(|_ack| PortError::Invariant)?,
                Err(PortError::Conflict) => {
                    // A competing start may have a different first recorded time.
                    let current = self.initial(&scope).await?;
                    let original_time = current.first().ok_or(PortError::Conflict)?.occurred_at;
                    let (original_commit, _) = self.plan(source, original_time)?;
                    let original_receipt = MachineSagaReceipt {
                        source,
                        consumer: self.consumer,
                        commit: original_commit,
                    };
                    self.verify_native(source, lease.payload(), &original_receipt)
                        .await?;
                    return Ok(original_receipt);
                }
                Err(error) => return Err(error.into()),
            }
        }
        self.verify_native(source, lease.payload(), &receipt)
            .await?;
        Ok(receipt)
    }
}
const fn verifier_error(error: &MachineSagaHandoffError) -> MachineStoreError {
    match error {
        MachineSagaHandoffError::Source(error) => *error,
        MachineSagaHandoffError::Port(
            PortError::Unavailable
            | PortError::TimedOut
            | PortError::Cancelled
            | PortError::Ambiguous,
        ) => MachineStoreError::Unavailable,
        MachineSagaHandoffError::Port(PortError::QuotaExceeded) => MachineStoreError::Capacity,
        MachineSagaHandoffError::Port(PortError::Unauthorized) => MachineStoreError::Denied,
        MachineSagaHandoffError::Port(PortError::Conflict) => MachineStoreError::Conflict,
        MachineSagaHandoffError::Port(PortError::Invariant)
        | MachineSagaHandoffError::Linear(_)
        | MachineSagaHandoffError::Graph(_)
        | MachineSagaHandoffError::Identifier(_) => MachineStoreError::Corrupt,
    }
}
#[async_trait]
impl<S: ProcessStore + ProcessPayloadStore + ProcessCommitVerifier, A: ProcessAuthorizer>
    MachineIntentAcceptor for MachineSagaReceiver<S, A>
{
    type Receipt = MachineSagaReceipt;
    fn identity(&self) -> MachineSourceId {
        self.consumer
    }
    async fn verify(
        &self,
        source: &MachineIntentSource,
        payload: &[u8],
        receipt: &MachineSagaReceipt,
    ) -> Result<MachineIntentAcceptance, MachineStoreError> {
        self.validate_source(*source, payload)
            .map_err(|error| verifier_error(&error))?;
        self.authorize(
            &self
                .scope(*source)
                .map_err(|error| verifier_error(&error))?,
        )
        .await
        .map_err(|error| verifier_error(&error))?;
        self.verify_native(*source, payload, receipt)
            .await
            .map_err(|error| verifier_error(&error))?;
        let mut bytes = b"penelope.machine-saga-receipt.v1\0".to_vec();
        bytes.extend_from_slice(&source.binding_identity(self.consumer));
        serde_json::to_writer(&mut bytes, &receipt.commit)
            .map_err(|_codec| MachineStoreError::Corrupt)?;
        Ok(MachineIntentAcceptance {
            source: *source,
            consumer: self.consumer,
            receipt_digest: ContentDigest::sha256(&bytes).0,
        })
    }
}
