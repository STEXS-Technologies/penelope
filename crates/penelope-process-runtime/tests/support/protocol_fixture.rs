//! Pure shared protocol fixture; no process execution or filesystem I/O.
use penelope_core::task::*;
use penelope_process_runtime::worker::WorkerMetadata;

pub const INPUT: &[u8] = &[0, 1, 2, 3];

pub fn metadata() -> WorkerMetadata {
    let definition = TaskDefinition {
        code: Digest([3; 32]),
        checkpoint_schema: 1,
        effects: EffectContract::Pure,
        recovery: TaskRecoveryPolicy {
            mode: RecoveryMode::RestartFromCheckpoint,
            max_attempts: 3,
            timeout_ticks: 50,
            restart_delay_ticks: 1,
            max_checkpoint_bytes: 64,
        },
        budget: ExecutionBudget {
            max_input_bytes: 64,
            max_output_bytes: 64,
            max_history_records: 32,
            max_checkpoint_commits: 8,
            cpu_ticks: 2,
            deadline_tick: TaskTick::new(1000),
        },
    };
    WorkerMetadata {
        scope: TaskScope {
            tenant: TaskTenantId::from_digest(Digest([1; 32])),
            task: TaskId::from_digest(Digest([2; 32])),
            definition: definition.fingerprint(),
            input: TaskInputDigest::from_digest(Digest::of(INPUT)),
        },
        generation: TaskGeneration::new(1),
        definition,
        execution_deadline: TaskTick::new(51),
        input_schema: 1,
    }
}

pub fn packet(metadata: &WorkerMetadata) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // Independent legacy heap-based digest encoding is also the wire oracle.
    let mut scope = b"penelope.linux-worker.scope.v1\0".to_vec();
    for digest in [
        metadata.scope.tenant.digest(),
        metadata.scope.task.digest(),
        metadata.scope.definition.digest(),
        metadata.scope.input.digest(),
    ] {
        scope.extend_from_slice(&digest.0);
    }
    let encoded = serde_json::to_vec(metadata)?;
    let mut bytes = b"PNLWRK01".to_vec();
    bytes.extend_from_slice(&Digest::of(&scope).0);
    for number in [
        metadata.generation.get(),
        u32::try_from(encoded.len())?,
        u32::try_from(INPUT.len())?,
        0,
        0,
    ] {
        bytes.extend_from_slice(&number.to_be_bytes());
    }
    bytes.extend_from_slice(&encoded);
    bytes.extend_from_slice(INPUT);
    Ok(bytes)
}
