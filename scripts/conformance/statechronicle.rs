//! Optional conformance against an installed StateChronicle checkout. The
//! authority/signing key and transactional event source are test fixtures;
//! signature verification, canonical encoding and event-root verification use
//! the actual upstream libraries. This is not a production StateChronicle client.
#![cfg(test)]
use ed25519_dalek::SigningKey;
use penelope_domain::*;
use penelope_ports::{payloads::ProcessPayloadStore, *};
use penelope_sqlite::{SqliteLeaseClock, SqliteLimits, SqliteProcessStore};
use penelope_statechronicle::{
    CanonicalCommandExpectation, bind_verified_event_to_commit, verify_committed_event,
};
use rusqlite::Connection;
use statechronicle_commit::{
    persist::SignedCommitVerifier,
    roots::event_root,
    sign::{Ed25519CommitVerifier, sign_commit},
};
use statechronicle_core::{
    canonicalize::{canonicalize, canonicalize_and_digest},
    digest::hash_bytes,
};
use statechronicle_domain::{
    commit::{Commit, CommitScope, ProfileId},
    event::{Event, StateCommitment},
    ids::{CommitId, EventId, IntentId},
    intent::{KeyId, Operation},
    resource::ResourceId as CanonicalResource,
    resource_state::{ResourceState, UniqueAssetState},
    signed::Signed,
    status::Status,
    subject::SubjectId,
    tenant::TenantId as CanonicalTenant,
};
use std::{
    future::Future,
    num::NonZeroU16,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};

type Error = Box<dyn std::error::Error>;
fn id<T: std::str::FromStr>(value: &str) -> T {
    value.parse().ok().unwrap()
}
struct Notify(std::thread::Thread);
impl Wake for Notify {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
fn run<T>(future: impl Future<Output = Result<T, PortError>>) -> Result<T, Error> {
    let waker = Waker::from(Arc::new(Notify(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return Ok(value?);
        }
        if Instant::now() >= deadline {
            return Err("database timeout".into());
        }
        std::thread::park_timeout(Duration::from_millis(1));
    }
}
struct Clock;
impl SqliteLeaseClock for Clock {
    fn now(&self) -> Result<LogicalTime, PortError> {
        Ok(LogicalTime(1))
    }
}
fn state(version: u64, owner: &str) -> StateCommitment {
    let state = ResourceState::UniqueAsset(UniqueAssetState {
        owner: SubjectId(owner.into()),
        status: Status::from_static("active"),
        trade_id: None,
    });
    StateCommitment {
        version,
        state_hash: canonicalize_and_digest(&state).unwrap(),
        state,
    }
}
fn event() -> Event {
    Event::new(
        CanonicalTenant("tnt_source".into()),
        EventId::new("evt_source".into()).unwrap(),
        IntentId::new("int_action".into()).unwrap(),
        Operation::new("asset.transfer".into()).unwrap(),
        CanonicalResource("res_source".into()),
        SubjectId("owner_before".into()),
        state(1, "owner_before"),
        state(2, "owner_after"),
        None,
        SubjectId("executor_fixture".into()),
        chrono::DateTime::parse_from_rfc3339("2026-10-30T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc),
    )
}
fn signed(event: &Event, key: &SigningKey) -> Signed<Commit> {
    let body = Commit::new(
        CommitScope::tenant(event.tenant_id.clone()),
        CommitId::new("cmt_source".into()).unwrap(),
        None,
        1,
        1,
        event_root(&[event.clone()]).unwrap(),
        hash_bytes(b"previous"),
        hash_bytes(b"next"),
        event.created_at,
        event.executor.clone(),
        ProfileId::new("statechronicle.profile.resource.v0".into()).unwrap(),
    );
    sign_commit(&body, key, KeyId::new("key_fixture".into()).unwrap()).unwrap()
}
// Source membership comes from a committed local transaction, while the signed
// commit binds exact event bytes. Neither a request response nor a bare hash is
// accepted as the source. The test key is trusted only for this tenant/executor.
fn verified_source(connection: &Connection, key: &SigningKey) -> Result<CanonicalEvent, Error> {
    let (signed, events): (Vec<u8>, Vec<u8>) = connection.query_row(
        "SELECT signed,events FROM committed_source WHERE id=1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let signed: Signed<Commit> = serde_json::from_slice(&signed)?;
    let events: Vec<Event> = serde_json::from_slice(&events)?;
    let verifier = Ed25519CommitVerifier::new(|commit: &Signed<Commit>| {
        if commit.body.scope != CommitScope::tenant(CanonicalTenant("tnt_source".into()))
            || commit.body.schema != statechronicle_domain::commit::COMMIT_SCHEMA
            || commit.body.profile.as_str() != "statechronicle.profile.resource.v0"
            || commit.signature.key_id.as_str() != "key_fixture"
            || commit.body.executor != SubjectId("executor_fixture".into())
        {
            return Err("untrusted tenant/key/executor".into());
        }
        Ok(key.verifying_key())
    });
    verifier
        .verify(&signed)
        .map_err(|error| format!("signature: {error}"))?;
    if signed.body.event_count != u64::try_from(events.len())?
        || event_root(&events)? != signed.body.event_merkle_root
    {
        return Err("event batch mismatch".into());
    }
    let event = events.first().ok_or("missing event")?;
    if events.len() != 1
        || event.schema != statechronicle_domain::event::EVENT_SCHEMA
        || event.tenant_id != CanonicalTenant("tnt_source".into())
        || event.intent_id.as_str() != "int_action"
        || event.operation.as_str() != "asset.transfer"
        || event.resource_id != CanonicalResource("res_source".into())
    {
        return Err("source command binding mismatch".into());
    }
    if canonicalize_and_digest(&event.before.state)? != event.before.state_hash
        || canonicalize_and_digest(&event.after.state)? != event.after.state_hash
    {
        return Err("state commitment mismatch".into());
    }
    Ok(CanonicalEvent {
        tenant_id: id("tnt_source"),
        source_event_id: id(&format!("cev_{}", event.event_id.as_str())),
        action_id: id("act_source"),
        commit_id: id(signed.body.commit_id.as_str()),
        commit_sequence: signed.body.sequence,
        operation: id("op_transfer"),
        resource_ids: vec![id("res_source")],
        payload_digest: ContentDigest::sha256(&canonicalize(event)?),
    })
}
fn write_source(
    connection: &mut Connection,
    commit: &Signed<Commit>,
    events: &[Event],
) -> Result<(), Error> {
    let transaction = connection.transaction()?;
    transaction.execute(
        "INSERT OR REPLACE INTO committed_source VALUES(1,?1,?2)",
        rusqlite::params![serde_json::to_vec(commit)?, serde_json::to_vec(events)?],
    )?;
    transaction.commit()?;
    Ok(())
}
#[test]
fn signed_committed_source_binds_atomic_commit_and_deduplicates_after_reopen() -> Result<(), Error>
{
    let directory = tempfile::tempdir()?;
    let source_path = directory.path().join("canonical.db");
    let mut source = Connection::open(&source_path)?;
    source.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; CREATE TABLE committed_source(id INTEGER PRIMARY KEY,signed BLOB NOT NULL,events BLOB NOT NULL)")?;
    let reader = Connection::open(&source_path)?;
    let event = event();
    let key = SigningKey::from_bytes(&[7; 32]);
    let commit = signed(&event, &key);
    {
        let pending = source.transaction()?;
        pending.execute(
            "INSERT INTO committed_source VALUES(1,?1,?2)",
            rusqlite::params![
                serde_json::to_vec(&commit)?,
                serde_json::to_vec(&vec![event.clone()])?
            ],
        )?;
        assert!(verified_source(&reader, &key).is_err());
        pending.rollback()?;
    }
    write_source(&mut source, &commit, &[event.clone()])?;
    let canonical = verified_source(&reader, &key)?;
    assert!(verified_source(&reader, &SigningKey::from_bytes(&[8; 32])).is_err());
    let mut changed = event.clone();
    changed.after = state(2, "attacker");
    write_source(&mut source, &commit, &[changed])?;
    assert!(verified_source(&reader, &key).is_err());
    let mut substituted = commit.clone();
    substituted.body.sequence = 99;
    write_source(&mut source, &substituted, &[event.clone()])?;
    assert!(verified_source(&reader, &key).is_err());
    let mut foreign = event.clone();
    foreign.intent_id = IntentId::new("int_other".into())?;
    write_source(&mut source, &signed(&foreign, &key), &[foreign])?;
    assert!(verified_source(&reader, &key).is_err());
    let mut foreign_commit = commit.body.clone();
    foreign_commit.scope = CommitScope::tenant(CanonicalTenant("tnt_other".into()));
    let foreign_signed = sign_commit(&foreign_commit, &key, KeyId::new("key_fixture".into())?)?;
    write_source(&mut source, &foreign_signed, &[event.clone()])?;
    assert!(verified_source(&reader, &key).is_err());
    write_source(&mut source, &commit, &[event.clone()])?;
    let scope = ProcessScope::new(
        id("tnt_source"),
        id("prc_source"),
        id("def_source"),
        id("dfv_one"),
        ContentDigest([9; 32]),
    );
    let expected = CanonicalCommandExpectation {
        scope: scope.clone(),
        action_id: id("act_source"),
        operation: id("op_transfer"),
        resource_ids: vec![id("res_source")],
        expected_event_payload_digest: ContentDigest::sha256(&canonicalize(&event)?),
    };
    let verified = verify_committed_event(&expected, canonical)?;
    let input = ProcessInput::new(
        scope.tenant_id.clone(),
        scope.process_id.clone(),
        id("inp_source"),
        ProcessInputKind::CanonicalEvent,
        verified.event.payload_digest,
    );
    let outcome = ProcessOutcome::new(
        scope.clone(),
        0,
        ProcessOutcomeFact::new(
            id("out_source"),
            CausationId::Input(input.input_id.clone()),
            OutcomeActor::System,
            LogicalTime(1),
            ProcessOutcomeKind::InputAccepted,
            verified.event.payload_digest,
        ),
    );
    let batch = bind_verified_event_to_commit(
        AtomicProcessCommit::new(0, Some(input), vec![outcome], Vec::new())?,
        &verified,
    )?;
    let path = directory.path().join("process.db");
    let open = || SqliteProcessStore::open(&path, SqliteLimits::default(), Arc::new(Clock), 100, 4);
    let process = open()?;
    // Immutable evidence bytes precede the process commit. These writes are
    // intentionally separate from the canonical-source transaction.
    assert_eq!(
        run(process.put_payload(
            &expected.scope,
            verified.event.payload_digest,
            &canonicalize(&event)?
        ))?,
        verified.event.payload_digest
    );
    let receipt = run(process.commit(&batch))?;
    receipt.validate_for(&batch)?;
    drop(process);
    let recovered = open()?;
    assert_eq!(
        run(recovered.read_payload(&expected.scope, verified.event.payload_digest))?,
        canonicalize(&event)?
    );
    let receipt = run(recovered.commit(&batch))?;
    receipt.validate_for(&batch)?;
    assert!(receipt.duplicate_input);
    let replay = OutcomeReplayRequest::new(scope, 0, NonZeroU16::new(4).unwrap())?;
    assert_eq!(
        run(recovered.read_outcomes(&replay))?.outcomes,
        batch.outcomes
    );
    let mut duplicate = batch.clone();
    duplicate.expected_sequence = 1;
    duplicate.input.as_mut().unwrap().input_id = id("inp_redelivered");
    duplicate.outcomes[0].sequence = 1;
    duplicate.outcomes[0].outcome_id = id("out_redelivered");
    duplicate.outcomes[0].causation_id = CausationId::Input(id("inp_redelivered"));
    assert_eq!(
        recovered_commit(&recovered, &duplicate),
        Err(PortError::Conflict)
    );
    Ok(())
}
fn recovered_commit(
    store: &SqliteProcessStore,
    commit: &AtomicProcessCommit,
) -> Result<AtomicProcessCommitReceipt, PortError> {
    // Preserve the exact typed error for the source-deduplication assertion.
    let result = run(store.commit(commit));
    match result {
        Ok(value) => Ok(value),
        Err(error) => Err(*error.downcast::<PortError>().unwrap()),
    }
}
