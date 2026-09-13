#![no_main]

use libfuzzer_sys::fuzz_target;
use penelope_domain::{
    ActionId, ContentDigest, DefinitionId, DefinitionVersion, ProcessActionKindV1, ProcessId,
    TenantId,
};
use penelope_executor::engine::{
    ActionResultObservationV1, ActionResultV1, CompensationPlanV1, LinearSagaDefinitionV1,
    LinearSagaEventEnvelopeV1, LinearSagaEventV1, RetryPolicyV1, StepPlanV1, apply_action_result,
    replay, replay_ordered, start,
};

fn identifier<T: TryFrom<&'static str>>(value: &'static str) -> T {
    match T::try_from(value) {
        Ok(identifier) => identifier,
        Err(_) => unreachable!(),
    }
}

fuzz_target!(|data: &[u8]| {
    let _ = serde_json::from_slice::<LinearSagaEventEnvelopeV1>(data);
    let _ = serde_json::from_slice::<LinearSagaDefinitionV1>(data);
    let step_count = data.first().map_or(0, |byte| usize::from(byte % 4));
    let definition = LinearSagaDefinitionV1 {
        definition_id: identifier::<DefinitionId>("def_fuzz"),
        definition_version: identifier::<DefinitionVersion>("dfv_one"),
        definition_digest: ContentDigest([99; 32]),
        steps: (0..step_count)
            .map(|index| StepPlanV1 {
                step_id: match index {
                    0 => identifier("stp_zero"),
                    1 => identifier("stp_one"),
                    2 => identifier("stp_two"),
                    _ => identifier("stp_three"),
                },
                action_kind: ProcessActionKindV1::CanonicalCommand,
                payload_digest: ContentDigest([index as u8; 32]),
                retry_policy: RetryPolicyV1::no_retry(),
                compensation: Some(CompensationPlanV1::canonical_command(
                    ContentDigest([u8::MAX - index as u8; 32]),
                    RetryPolicyV1::no_retry(),
                )),
            })
            .collect(),
    };
    let tenant_id = identifier::<TenantId>("tnt_fuzz");
    let process_id = identifier::<ProcessId>("prc_fuzz");
    let action_id = identifier::<ActionId>("act_fuzz_start");
    let replay_events = [
        LinearSagaEventV1::Started {
            action_id: action_id.clone(),
        },
        LinearSagaEventV1::ActionResultObserved {
            observation: ActionResultObservationV1::succeeded(action_id.clone()),
            next_action_id: Some(identifier("act_fuzz_next")),
        },
    ];
    let _ = replay(&definition, &tenant_id, &process_id, &replay_events);
    let ordered_replay_events = [
        LinearSagaEventEnvelopeV1 {
            sequence: 0,
            event: replay_events[0].clone(),
        },
        LinearSagaEventEnvelopeV1 {
            sequence: 1,
            event: replay_events[1].clone(),
        },
    ];
    let _ = replay_ordered(&definition, &tenant_id, &process_id, &ordered_replay_events);
    let Ok(mut decision) = start(
        &definition,
        tenant_id.clone(),
        process_id.clone(),
        action_id,
    ) else {
        return;
    };

    for byte in data.iter().skip(1) {
        let result = match byte % 4 {
            0 => ActionResultV1::Succeeded,
            1 => ActionResultV1::RetryableFailure,
            2 => ActionResultV1::TerminalFailure,
            _ => ActionResultV1::Unknown,
        };
        let next_action_id = match result {
            ActionResultV1::Succeeded | ActionResultV1::RetryableFailure => {
                Some(identifier("act_fuzz_next"))
            }
            ActionResultV1::TerminalFailure | ActionResultV1::Unknown => None,
        };
        let Some(active_action) = decision.next_action.as_ref() else {
            return;
        };
        let observation = ActionResultObservationV1 {
            action_id: active_action.action_id.clone(),
            result,
        };
        let Ok(next) = apply_action_result(
            &definition,
            &decision.projection,
            tenant_id.clone(),
            process_id.clone(),
            &observation,
            next_action_id,
        ) else {
            return;
        };
        decision = next;
    }
});
