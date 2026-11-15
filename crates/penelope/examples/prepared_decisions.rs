//! One inspection interface; saga and task admission keep their own authorities.
//! This example prepares proposals only. Production applications use the existing
//! atomic process commit or task journal before dispatching or starting work.
#[path = "support/inspection.rs"]
mod inspection;
use penelope::{
    ContentDigest, ProcessId, TenantId,
    saga::{LinearSagaDefinition, LinearSagaInput, RetryPolicy, StepPlan, prepare_linear},
    task::*,
};
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let definition = LinearSagaDefinition::new(
        "def_example".try_into()?,
        "dfv_one".try_into()?,
        ContentDigest([1; 32]),
        vec![StepPlan::canonical_command(
            "stp_work".try_into()?,
            ContentDigest([2; 32]),
            RetryPolicy::no_retry(),
        )],
    );
    let tenant: TenantId = "tnt_example".try_into()?;
    let process: ProcessId = "prc_example".try_into()?;
    let input = LinearSagaInput::Start {
        input_id: "inp_start".try_into()?,
        action_id: "act_work".try_into()?,
    };
    let saga = prepare_linear(&definition, None, &tenant, &process, &input)?;
    inspection::inspect("saga", &saga);
    let (_event, decision) = saga.into_parts();
    println!(
        "saga candidate={:?}; atomic admission required",
        decision.projection.status
    );

    let input_bytes = [1, 2];
    let task_definition = TaskDefinition {
        code: Digest([3; 32]),
        checkpoint_schema: 1,
        effects: EffectContract::Pure,
        recovery: TaskRecoveryPolicy {
            mode: RecoveryMode::NeverRestart,
            max_attempts: 1,
            timeout_ticks: 10,
            restart_delay_ticks: 1,
            max_checkpoint_bytes: 8,
        },
        budget: ExecutionBudget {
            max_input_bytes: 8,
            max_output_bytes: 8,
            max_history_records: 8,
            max_checkpoint_commits: 0,
            cpu_ticks: 10,
            deadline_tick: TaskTick::new(100),
        },
    };
    let scope = TaskScope {
        tenant: TaskTenantId::from_digest(Digest([4; 32])),
        task: TaskId::from_digest(Digest([5; 32])),
        definition: task_definition.fingerprint(),
        input: TaskInputDigest::from_digest(Digest::of(&input_bytes)),
    };
    let projection = TaskProjection::new(scope, task_definition)?;
    let task = projection.prepare(TaskRecord {
        version: 1,
        scope,
        sequence: projection.next_sequence(),
        generation: TaskGeneration::ZERO,
        tick: TaskTick::ZERO,
        event: TaskExecutionEvent::IntentCommitted {
            definition: task_definition,
            input: BlobRef {
                digest: scope.input.digest(),
                bytes: 2,
                schema: 1,
            },
        },
    })?;
    inspection::inspect("task", &task);
    let record = task.into_record();
    println!(
        "task proposal sequence={}; journal admission required",
        record.sequence.get()
    );
    Ok(())
}
