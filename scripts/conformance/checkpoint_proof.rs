//! A test authority commits a custom checkpoint state leaf. Actual upstream
//! Ed25519 and sparse-Merkle verification precede Penelope checkpoint binding.
//! Signer/head trust and publication are fixtures, not an upstream execution client.
use super::*;
use penelope_core::task::{runtime::*, *};
use penelope_executor::task::{SupervisorError, TaskSupervisor};
use penelope_sqlite::SqliteTaskJournal;
use penelope_statechronicle::task::{
    CheckpointStateError, CheckpointStateEvidence, CheckpointStateVerifier, verify_checkpoint_state,
};
use statechronicle_accumulator::{
    key::StateKey,
    sparse_merkle::{StateAccumulator, StateUpdate},
};
use statechronicle_domain::{
    custom::CustomState, proof::ResourceStateProof, state::StateProjection, state_type::StateType,
};
use statechronicle_proof::{bundle::build_state_proof, verify::verify_bundle};
use std::path::{Path, PathBuf};

const DOMAIN: &str = "penelope.checkpoint.v1";
const VERSION: u64 = 7;
const INPUT: [u8; 4] = [5, 1, 2, 3];
#[derive(Clone)]
struct Policy {
    path: PathBuf,
    key: ed25519_dalek::VerifyingKey,
    head_digest: statechronicle_core::digest::ContentDigest,
    state_version: u64,
}
impl CheckpointStateVerifier for Policy {
    fn verify(&self, record: &TaskRecord) -> Result<CheckpointStateEvidence, CheckpointStateError> {
        self.verified(record)
            .map_err(|_| CheckpointStateError::VerificationFailed)
    }
}
impl Policy {
    fn verified(&self, record: &TaskRecord) -> Result<CheckpointStateEvidence, Error> {
        // Source presence is checked independently of the task database. A request
        // response, proposed root, or uncommitted source row is not evidence.
        let connection = Connection::open(&self.path)?;
        let (commit, proof): (Vec<u8>, Vec<u8>) = connection.query_row(
            "SELECT signed,proof FROM checkpoint_source WHERE id=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let signed: Signed<Commit> = serde_json::from_slice(&commit)?;
        let proof: ResourceStateProof = serde_json::from_slice(&proof)?;
        if canonicalize_and_digest(&signed.body)? != self.head_digest
            || signed.body.scope != CommitScope::tenant(CanonicalTenant("tnt_checkpoint".into()))
            || signed.body.schema != statechronicle_domain::commit::COMMIT_SCHEMA
            || signed.body.profile.as_str() != DOMAIN
            || signed.signature.key_id.as_str() != "key_checkpoint"
            || signed.body.executor != SubjectId("checkpoint_authority".into())
            || proof.tenant_id != CanonicalTenant("tnt_checkpoint".into())
            || proof.resource_id != CanonicalResource("res_checkpoint".into())
        {
            return Err("untrusted or stale checkpoint authority".into());
        }
        let key = StateKey::for_resource("tnt_checkpoint", "res_checkpoint");
        verify_bundle(&proof, &signed, &self.key, &key)?;
        let ResourceState::Custom(state) = &proof.claimed_state else {
            return Err("not a checkpoint domain".into());
        };
        if state.domain.as_str() != DOMAIN
            || state.schema_version != 1
            || state.payload != canonicalize(&(self.state_version, record))?
        {
            return Err("checkpoint/state version binding mismatch".into());
        }
        let checkpoint = match record.event {
            TaskExecutionEvent::CheckpointCommitted { checkpoint } => checkpoint,
            _ => return Err("not a checkpoint commit".into()),
        };
        Ok(CheckpointStateEvidence {
            scope: record.scope,
            generation: record.generation,
            checkpoint,
            state_version: self.state_version,
            state_digest: Digest(*canonicalize_and_digest(&proof.claimed_state)?.as_bytes()),
            proof_digest: Digest::of(&canonicalize(&(proof, signed))?),
        })
    }
}
fn task_scope(definition: TaskDefinition) -> TaskScope {
    TaskScope {
        tenant: TaskTenantId::from_digest(Digest::of(b"tnt_checkpoint")),
        task: TaskId::from_digest(Digest::of(b"task_checkpoint")),
        definition: definition.fingerprint(),
        input: TaskInputDigest::from_digest(Digest::of(&INPUT)),
    }
}
fn checkpoint_record(definition: TaskDefinition) -> TaskRecord {
    let scope = task_scope(definition);
    let mut projection = TaskProjection::new(scope, definition).unwrap();
    for (event, generation, tick) in [
        (
            TaskExecutionEvent::IntentCommitted {
                definition,
                input: BlobRef {
                    digest: Digest::of(&INPUT),
                    bytes: 4,
                    schema: 1,
                },
            },
            TaskGeneration::ZERO,
            TaskTick::ZERO,
        ),
        (
            TaskExecutionEvent::Started,
            TaskGeneration::new(1),
            TaskTick::new(1),
        ),
    ] {
        let record = projection.propose(event, generation, tick).unwrap();
        projection.apply(&record).unwrap();
    }
    projection
        .propose(
            TaskExecutionEvent::CheckpointCommitted {
                checkpoint: BlobRef {
                    digest: Digest::of(&6_u64.to_le_bytes()),
                    bytes: 8,
                    schema: 1,
                },
            },
            TaskGeneration::new(1),
            TaskTick::new(2),
        )
        .unwrap()
}
fn proof(record: &TaskRecord) -> (Signed<Commit>, ResourceStateProof, SigningKey) {
    proof_with_tags(record, DOMAIN, 1)
}
fn proof_with_tags(
    record: &TaskRecord,
    domain: &str,
    schema: u32,
) -> (Signed<Commit>, ResourceStateProof, SigningKey) {
    let state = ResourceState::Custom(CustomState {
        domain: ProfileId::new(domain.into()).unwrap(),
        schema_version: schema,
        payload: canonicalize(&(VERSION, record)).unwrap(),
    });
    let state_hash = canonicalize_and_digest(&state).unwrap();
    let key = StateKey::for_resource("tnt_checkpoint", "res_checkpoint");
    let mut accumulator = StateAccumulator::empty();
    accumulator
        .insert_batch(&[StateUpdate::new(key, *state_hash.as_bytes())])
        .unwrap();
    let key_pair = SigningKey::from_bytes(&[11; 32]);
    let body = Commit::new(
        CommitScope::tenant(CanonicalTenant("tnt_checkpoint".into())),
        CommitId::new("cmt_checkpoint".into()).unwrap(),
        None,
        1,
        1,
        hash_bytes(b"checkpoint-event"),
        hash_bytes(b"previous"),
        statechronicle_core::digest::ContentDigest::new(*accumulator.root().as_bytes()),
        event().created_at,
        SubjectId("checkpoint_authority".into()),
        ProfileId::new(DOMAIN.into()).unwrap(),
    );
    let signed = sign_commit(
        &body,
        &key_pair,
        KeyId::new("key_checkpoint".into()).unwrap(),
    )
    .unwrap();
    let projection = StateProjection {
        tenant_id: CanonicalTenant("tnt_checkpoint".into()),
        resource_id: CanonicalResource("res_checkpoint".into()),
        state_type: StateType::Custom,
        version: VERSION,
        last_event_id: EventId::new("evt_checkpoint".into()).unwrap(),
        last_commit_id: body.commit_id.clone(),
        state_hash,
        state,
    };
    let proof = build_state_proof(
        &projection,
        &signed,
        &accumulator.prove_inclusion(&key).unwrap(),
        &Operation::new("checkpoint.publish".into()).unwrap(),
        None,
        key,
    )
    .unwrap();
    (signed, proof, key_pair)
}
fn publish(path: &Path, signed: &Signed<Commit>, proof: &ResourceStateProof) -> Policy {
    let mut connection = Connection::open(path).unwrap();
    connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; CREATE TABLE checkpoint_source(id INTEGER PRIMARY KEY,signed BLOB NOT NULL,proof BLOB NOT NULL)").unwrap();
    let transaction = connection.transaction().unwrap();
    transaction
        .execute(
            "INSERT INTO checkpoint_source VALUES(1,?1,?2)",
            rusqlite::params![
                serde_json::to_vec(signed).unwrap(),
                serde_json::to_vec(proof).unwrap()
            ],
        )
        .unwrap();
    transaction.commit().unwrap();
    Policy {
        path: path.into(),
        key: SigningKey::from_bytes(&[11; 32]).verifying_key(),
        head_digest: canonicalize_and_digest(&signed.body).unwrap(),
        state_version: VERSION,
    }
}
#[test]
fn signed_checkpoint_proof_rejects_substitution_and_stale_authority() {
    let record = checkpoint_record(definition(2));
    let (signed, bundle, _) = proof(&record);
    let directory = tempfile::tempdir().unwrap();
    let policy = publish(&directory.path().join("canonical.db"), &signed, &bundle);
    let evidence = verify_checkpoint_state(&record, &policy).unwrap();
    assert_eq!(evidence.state_version, VERSION);
    let mut source = Connection::open(&policy.path).unwrap();
    source.execute("DELETE FROM checkpoint_source", []).unwrap();
    let pending = source.transaction().unwrap();
    pending
        .execute(
            "INSERT INTO checkpoint_source VALUES(1,?1,?2)",
            rusqlite::params![
                serde_json::to_vec(&signed).unwrap(),
                serde_json::to_vec(&bundle).unwrap()
            ],
        )
        .unwrap();
    assert!(verify_checkpoint_state(&record, &policy).is_err());
    pending.commit().unwrap();
    assert_eq!(verify_checkpoint_state(&record, &policy).unwrap(), evidence);
    for altered in [
        TaskRecord {
            generation: TaskGeneration::new(2),
            ..record
        },
        TaskRecord {
            scope: TaskScope {
                input: TaskInputDigest::from_digest(Digest([9; 32])),
                ..record.scope
            },
            ..record
        },
        TaskRecord {
            event: TaskExecutionEvent::CheckpointCommitted {
                checkpoint: BlobRef {
                    digest: Digest([9; 32]),
                    bytes: 8,
                    schema: 1,
                },
            },
            ..record
        },
        TaskRecord {
            tick: TaskTick::new(3),
            ..record
        },
    ] {
        assert!(verify_checkpoint_state(&altered, &policy).is_err());
    }
    for scope in [
        TaskScope {
            tenant: TaskTenantId::from_digest(Digest([9; 32])),
            ..record.scope
        },
        TaskScope {
            task: TaskId::from_digest(Digest([9; 32])),
            ..record.scope
        },
        TaskScope {
            definition: TaskDefinitionFingerprint::from_digest(Digest([9; 32])),
            ..record.scope
        },
    ] {
        assert!(verify_checkpoint_state(&TaskRecord { scope, ..record }, &policy).is_err());
    }
    let checkpoint = match record.event {
        TaskExecutionEvent::CheckpointCommitted { checkpoint } => checkpoint,
        _ => unreachable!(),
    };
    for checkpoint in [
        BlobRef {
            bytes: 9,
            ..checkpoint
        },
        BlobRef {
            schema: 2,
            ..checkpoint
        },
    ] {
        assert!(
            verify_checkpoint_state(
                &TaskRecord {
                    event: TaskExecutionEvent::CheckpointCommitted { checkpoint },
                    ..record
                },
                &policy
            )
            .is_err()
        );
    }
    for altered in [
        TaskRecord {
            sequence: TaskSequence::new(99),
            ..record
        },
        TaskRecord {
            version: 2,
            ..record
        },
    ] {
        assert!(verify_checkpoint_state(&altered, &policy).is_err());
    }
    let mut wrong_key = policy.clone();
    wrong_key.key = SigningKey::from_bytes(&[12; 32]).verifying_key();
    assert!(verify_checkpoint_state(&record, &wrong_key).is_err());
    let mut stale = policy.clone();
    stale.head_digest = hash_bytes(b"different authoritative head");
    assert!(verify_checkpoint_state(&record, &stale).is_err());
    let mut wrong_version = policy.clone();
    wrong_version.state_version = 8;
    assert!(verify_checkpoint_state(&record, &wrong_version).is_err());
    for (domain, schema) in [("penelope.other.v1", 1), (DOMAIN, 2)] {
        let (tagged_commit, tagged_proof, _) = proof_with_tags(&record, domain, schema);
        let temporary = tempfile::tempdir().unwrap();
        let tagged = publish(
            &temporary.path().join("canonical.db"),
            &tagged_commit,
            &tagged_proof,
        );
        // The cryptographic proof is valid: application tags still reject it.
        verify_bundle(
            &tagged_proof,
            &tagged_commit,
            &tagged.key,
            &StateKey::for_resource("tnt_checkpoint", "res_checkpoint"),
        )
        .unwrap();
        assert!(verify_checkpoint_state(&record, &tagged).is_err());
    }
    let forged = sign_commit(
        &signed.body,
        &SigningKey::from_bytes(&[12; 32]),
        KeyId::new("key_checkpoint".into()).unwrap(),
    )
    .unwrap();
    let connection = Connection::open(&policy.path).unwrap();
    connection
        .execute(
            "UPDATE checkpoint_source SET signed=?1",
            [serde_json::to_vec(&forged).unwrap()],
        )
        .unwrap();
    assert!(verify_checkpoint_state(&record, &policy).is_err());
    connection
        .execute(
            "UPDATE checkpoint_source SET signed=?1",
            [serde_json::to_vec(&signed).unwrap()],
        )
        .unwrap();
    // Tampering source bytes must fail despite a still-valid signed commit.
    for damage in 0..4 {
        let mut changed = bundle.clone();
        match damage {
            0 => {
                if let ResourceState::Custom(state) = &mut changed.claimed_state {
                    state.payload[0] ^= 1;
                }
            }
            1 => changed.state_inclusion_proof.path.clear(),
            2 => changed.resource_id = CanonicalResource("res_other".into()),
            _ => changed.tenant_id = CanonicalTenant("tnt_other".into()),
        }
        let connection = Connection::open(&policy.path).unwrap();
        connection
            .execute(
                "UPDATE checkpoint_source SET proof=?1",
                [serde_json::to_vec(&changed).unwrap()],
            )
            .unwrap();
        assert!(verify_checkpoint_state(&record, &policy).is_err());
    }
}
struct AckRuntime {
    inner: Runtime,
    attempts: usize,
}
impl TaskRuntime for AckRuntime {
    fn start(&mut self, request: TaskStart<'_>) -> Result<(), TaskPortError> {
        self.inner.start(request)
    }
    fn poll(
        &mut self,
        scope: TaskScope,
        generation: TaskGeneration,
        buffer: &mut [u8],
    ) -> Result<TaskObservation, TaskPortError> {
        self.inner.poll(scope, generation, buffer)
    }
    fn acknowledge_checkpoint(
        &mut self,
        scope: TaskScope,
        generation: TaskGeneration,
        checkpoint: BlobRef,
    ) -> Result<(), TaskPortError> {
        self.attempts = self
            .attempts
            .checked_add(1)
            .ok_or(TaskPortError::Exhausted)?;
        self.inner
            .acknowledge_checkpoint(scope, generation, checkpoint)
    }
    fn fence(&mut self, scope: TaskScope, generation: TaskGeneration) -> Result<(), TaskPortError> {
        self.inner.fence(scope, generation)
    }
}
struct ProofJournal {
    inner: SqliteTaskJournal,
    policy: Policy,
}
impl TaskJournal for ProofJournal {
    fn read(
        &mut self,
        scope: TaskScope,
        sequence: TaskSequence,
    ) -> Result<Option<TaskRecord>, TaskPortError> {
        self.inner.read(scope, sequence)
    }
    fn read_blob(
        &mut self,
        scope: TaskScope,
        blob: BlobRef,
        buffer: &mut [u8],
    ) -> Result<usize, TaskPortError> {
        self.inner.read_blob(scope, blob, buffer)
    }
    fn commit(
        &mut self,
        record: &TaskRecord,
        blob: Option<&[u8]>,
    ) -> Result<TaskRecord, TaskPortError> {
        if matches!(record.event, TaskExecutionEvent::CheckpointCommitted { .. }) {
            verify_checkpoint_state(record, &self.policy)
                .map_err(|_| TaskPortError::Unavailable)?;
        }
        self.inner.commit(record, blob)
    }
}
#[test]
fn signed_checkpoint_proof_gates_real_worker_commit_ack_and_restore() {
    for accepted in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let definition = definition(2);
        let scope = task_scope(definition);
        let record = checkpoint_record(definition);
        let (signed, bundle, _) = proof(&record);
        let mut policy = publish(&directory.path().join("canonical.db"), &signed, &bundle);
        if !accepted {
            policy.key = SigningKey::from_bytes(&[12; 32]).verifying_key();
        }
        let path = directory.path().join("tasks.db");
        let open = || SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
        let journal = ProofJournal {
            inner: open(),
            policy,
        };
        let runtime = AckRuntime {
            inner: Runtime::new(open(), config(directory.path())).unwrap(),
            attempts: 0,
        };
        let mut supervisor: TaskSupervisor<_, _, 64> =
            TaskSupervisor::restore(journal, runtime, scope, definition, TaskTick::ZERO).unwrap();
        supervisor.submit(&INPUT, 1, TaskTick::ZERO).unwrap();
        supervisor.advance(TaskTick::new(1)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let result = loop {
            let result = supervisor.poll(TaskTick::new(2));
            if result != Ok(false) {
                break result;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        };
        if accepted {
            assert_eq!(result, Ok(true));
            assert_eq!(
                supervisor.projection().checkpoint(),
                match record.event {
                    TaskExecutionEvent::CheckpointCommitted { checkpoint } => Some(checkpoint),
                    _ => None,
                }
            );
            while supervisor.projection().status() == TaskStatus::Running {
                supervisor.poll(TaskTick::new(3)).unwrap();
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            assert_eq!(supervisor.projection().status(), TaskStatus::Completed);
        } else {
            assert_eq!(
                result,
                Err(SupervisorError::Port(TaskPortError::Unavailable))
            );
            assert!(supervisor.projection().checkpoint().is_none());
            assert!(supervisor.projection().output().is_none());
            assert_eq!(
                supervisor.advance(TaskTick::new(3)),
                Err(SupervisorError::ReconstructionRequired)
            );
        }
        let (mut journal, runtime) = supervisor.into_parts();
        assert_eq!(runtime.attempts, usize::from(accepted));
        if accepted {
            assert_eq!(
                journal.inner.read(scope, TaskSequence::new(2)).unwrap(),
                Some(record)
            );
        } else {
            assert!(
                journal
                    .inner
                    .read(scope, TaskSequence::new(2))
                    .unwrap()
                    .is_none()
            );
            let checkpoint = match record.event {
                TaskExecutionEvent::CheckpointCommitted { checkpoint } => checkpoint,
                _ => unreachable!(),
            };
            assert!(
                journal
                    .inner
                    .read_blob(scope, checkpoint, &mut [0; 8])
                    .is_err()
            );
        }
        let restored: TaskSupervisor<_, _, 64> =
            TaskSupervisor::restore(journal, runtime, scope, definition, TaskTick::new(4)).unwrap();
        assert_eq!(
            restored.projection().status(),
            if accepted {
                TaskStatus::Completed
            } else {
                TaskStatus::Faulted
            }
        );
        if !accepted {
            assert!(restored.projection().checkpoint().is_none());
        }
    }
}
