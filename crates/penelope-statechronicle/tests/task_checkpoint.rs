#![allow(clippy::unwrap_used)]
use penelope_ports::task::*;
use penelope_statechronicle::task::*;

#[test]
fn verified_checkpoint_state_cannot_be_reused_across_generation_or_scope() {
    let checkpoint = BlobRef {
        digest: Digest([4; 32]),
        bytes: 12,
        schema: 1,
    };
    let scope = TaskScope {
        tenant: Digest([1; 32]),
        task: Digest([2; 32]),
        definition: Digest([3; 32]),
        input: Digest([5; 32]),
    };
    let record = TaskRecord {
        version: 1,
        scope,
        sequence: 4,
        generation: 2,
        tick: 10,
        event: TaskExecutionEvent::CheckpointCommitted { checkpoint },
    };
    let evidence = CheckpointStateEvidence {
        scope,
        generation: 2,
        checkpoint,
        state_version: 7,
        state_digest: Digest([8; 32]),
        proof_digest: Digest([9; 32]),
    };
    assert_eq!(bind_checkpoint_state(&record, evidence).unwrap(), evidence);
    let stale = CheckpointStateEvidence {
        generation: 1,
        ..evidence
    };
    assert_eq!(
        bind_checkpoint_state(&record, stale),
        Err(CheckpointStateError::BindingMismatch)
    );
    let foreign = CheckpointStateEvidence {
        scope: TaskScope {
            tenant: Digest([6; 32]),
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
