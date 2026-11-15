//! One inspection interface; machine, saga and task retain their own authorities.
//! This example prepares proposals only. Production applications use the existing
//! atomic process commit or task journal before dispatching or starting work.
//! Machine journal admission remains under development; this demo aborts its batch.
#[path = "support/counter.rs"]
mod counter;
#[path = "support/counter_codec.rs"]
mod counter_codec;
#[path = "support/inspection.rs"]
mod inspection;
use penelope::{
    ContentDigest, ProcessId, TenantId,
    machine::*,
    saga::{LinearSagaDefinition, LinearSagaInput, RetryPolicy, StepPlan, prepare_linear},
    storage::machine::*,
    task::*,
};
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    prepare_counter()?;
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

fn prepare_counter() -> Result<(), Box<dyn Error>> {
    let scope = MachineScope {
        tenant: MachineTenantId::from_bytes([1; 32]),
        instance: MachineId::from_bytes([2; 32]),
        definition: counter::DEFINITION,
    };
    let mut owner = MemoryMachine::<_, 2>::new(
        counter::Counter,
        scope,
        0,
        IntentLimits {
            max_intents: 2,
            max_payload_bytes: 16,
        },
    )?;
    let event = 3_u64;
    let context = TransitionContext {
        expected: owner.read().0,
        event: MachineEventId::from_bytes([4; 32]),
        event_digest: Digest::of(&event.to_le_bytes()).0,
        logical_time: MachineLogicalTime::new(1),
    };
    let mut state_bytes = [0; 8];
    let mut event_bytes = [0; 8];
    let mut intent_bytes = [0; 8];
    let decision = prepare_machine_commit(
        &mut owner,
        &MachineCommitInput {
            event: &event,
            context,
            codec: &counter_codec::CounterCodec,
            causation: None,
        },
        &mut MachineCommitBuffers {
            state: &mut state_bytes,
            event: &mut event_bytes,
            intent: &mut intent_bytes,
        },
        MachinePayloadLimits {
            max_record_bytes: 512,
            max_event_bytes: 8,
            max_state_bytes: 8,
            max_intent_bytes: 8,
            max_batch_bytes: 512,
        },
    )?;
    if let MachineCommitDecision::Prepared(prepared) = decision {
        inspection::inspect("machine commit", &prepared);
        println!(
            "machine batch={} bytes; no journal admission",
            prepared.batch().byte_len()
        );
        drop(prepared); // No fabricated receipt and no local publication.
    }
    println!("machine remains={}", owner.read().1);
    Ok(())
}
