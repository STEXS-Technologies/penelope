#![allow(clippy::unwrap_used)]
use penelope_ports::task::*;
use penelope_statechronicle::task::*;

const fn fixture() -> (TaskRecord, CheckpointStateEvidence) {
    let checkpoint = BlobRef {
        digest: Digest([4; 32]),
        bytes: 12,
        schema: 1,
    };
    let scope = TaskScope {
        tenant: TaskTenantId::from_digest(Digest([1; 32])),
        task: TaskId::from_digest(Digest([2; 32])),
        definition: TaskDefinitionFingerprint::from_digest(Digest([3; 32])),
        input: TaskInputDigest::from_digest(Digest([5; 32])),
    };
    let record = TaskRecord {
        version: 1,
        scope,
        sequence: TaskSequence::new(4),
        generation: TaskGeneration::new(2),
        tick: TaskTick::new(10),
        event: TaskExecutionEvent::CheckpointCommitted { checkpoint },
    };
    let evidence = CheckpointStateEvidence {
        scope,
        generation: TaskGeneration::new(2),
        checkpoint,
        state_version: 7,
        state_digest: Digest([8; 32]),
        proof_digest: Digest([9; 32]),
    };
    (record, evidence)
}

#[test]
fn verified_checkpoint_state_cannot_be_reused_across_generation_or_scope() {
    let (record, evidence) = fixture();
    let scope = record.scope;
    let checkpoint = evidence.checkpoint;
    assert_eq!(bind_checkpoint_state(&record, evidence).unwrap(), evidence);
    let stale = CheckpointStateEvidence {
        generation: TaskGeneration::new(1),
        ..evidence
    };
    assert_eq!(
        bind_checkpoint_state(&record, stale),
        Err(CheckpointStateError::BindingMismatch)
    );
    let foreign = CheckpointStateEvidence {
        scope: TaskScope {
            tenant: TaskTenantId::from_digest(Digest([6; 32])),
            ..scope
        },
        ..evidence
    };
    assert_eq!(
        bind_checkpoint_state(&record, foreign),
        Err(CheckpointStateError::BindingMismatch)
    );
    let wrong_blob = CheckpointStateEvidence {
        checkpoint: BlobRef {
            schema: 2,
            ..checkpoint
        },
        ..evidence
    };
    assert_eq!(
        bind_checkpoint_state(&record, wrong_blob),
        Err(CheckpointStateError::BindingMismatch)
    );
}

struct Verifier {
    expected: TaskRecord,
    result: Result<CheckpointStateEvidence, CheckpointStateError>,
    calls: std::cell::Cell<u32>,
}
impl CheckpointStateVerifier for Verifier {
    fn verify(&self, record: &TaskRecord) -> Result<CheckpointStateEvidence, CheckpointStateError> {
        self.calls.set(self.calls.get().saturating_add(1));
        if record != &self.expected {
            return Err(CheckpointStateError::VerificationFailed);
        }
        self.result
    }
}

#[test]
fn combined_verification_invokes_the_verifier_and_propagates_failure() {
    let (record, evidence) = fixture();
    for result in [Ok(evidence), Err(CheckpointStateError::VerificationFailed)] {
        let verifier = Verifier {
            expected: record,
            result,
            calls: std::cell::Cell::new(0),
        };
        assert_eq!(verify_checkpoint_state(&record, &verifier), result);
        assert_eq!(verifier.calls.get(), 1);
    }
}

#[test]
fn returned_evidence_must_match_every_scope_generation_and_blob_coordinate() {
    let (record, evidence) = fixture();
    for case in 0..8 {
        let mut substituted = evidence;
        match case {
            0 => substituted.scope.tenant = TaskTenantId::from_digest(Digest([99; 32])),
            1 => substituted.scope.task = TaskId::from_digest(Digest([99; 32])),
            2 => {
                substituted.scope.definition =
                    TaskDefinitionFingerprint::from_digest(Digest([99; 32]))
            }
            3 => substituted.scope.input = TaskInputDigest::from_digest(Digest([99; 32])),
            4 => substituted.generation = TaskGeneration::new(3),
            5 => substituted.checkpoint.digest = Digest([99; 32]),
            6 => substituted.checkpoint.bytes = 13,
            _ => substituted.checkpoint.schema = 2,
        }
        let verifier = Verifier {
            expected: record,
            result: Ok(substituted),
            calls: std::cell::Cell::new(0),
        };
        assert_eq!(
            verify_checkpoint_state(&record, &verifier),
            Err(CheckpointStateError::BindingMismatch)
        );
        assert_eq!(verifier.calls.get(), 1);
    }
}

#[test]
fn invalid_records_do_not_invoke_the_independent_verifier() {
    let (record, evidence) = fixture();
    let verifier = Verifier {
        expected: record,
        result: Ok(evidence),
        calls: std::cell::Cell::new(0),
    };
    let unsupported = TaskRecord {
        version: 2,
        ..record
    };
    assert_eq!(
        verify_checkpoint_state(&unsupported, &verifier),
        Err(CheckpointStateError::BindingMismatch)
    );
    let not_checkpoint = TaskRecord {
        event: TaskExecutionEvent::Started,
        ..record
    };
    assert_eq!(
        verify_checkpoint_state(&not_checkpoint, &verifier),
        Err(CheckpointStateError::NotCheckpoint)
    );
    assert_eq!(verifier.calls.get(), 0);
}

#[test]
fn migrated_checkpoint_verification_binds_the_new_blob_and_generation() {
    let (original, evidence) = fixture();
    let migrated = TaskRecord {
        event: TaskExecutionEvent::CheckpointMigrated {
            source: TaskScope {
                task: TaskId::from_digest(Digest([88; 32])),
                ..original.scope
            },
            previous: BlobRef {
                digest: Digest([77; 32]),
                schema: 2,
                ..evidence.checkpoint
            },
            checkpoint: evidence.checkpoint,
            authorization: Digest([66; 32]),
        },
        ..original
    };
    let verifier = Verifier {
        expected: migrated,
        result: Ok(evidence),
        calls: std::cell::Cell::new(0),
    };
    assert_eq!(verify_checkpoint_state(&migrated, &verifier), Ok(evidence));
    assert_eq!(verifier.calls.get(), 1);
}
