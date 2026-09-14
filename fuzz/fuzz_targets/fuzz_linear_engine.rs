#![no_main]

use libfuzzer_sys::fuzz_target;
use penelope_domain::{
    ActionId, CausationIdV1, ContentDigest, DefinitionId, DefinitionVersion, InputId,
    LogicalTimeV1, OutcomeActorV1, OutcomeId, ProcessActionKindV1, ProcessId, ProcessOutcomeFactV1,
    ProcessScopeV1, TenantId,
};
use penelope_executor::engine::{
    ActionResultObservationV1, ActionResultV1, CompensationPlanV1, LinearSagaDefinitionV1,
    LinearSagaEventEnvelopeV1, LinearSagaEventV1, LinearSagaInputV1, RetryBackoffV1,
    RetryJitterSeedV1, RetryPolicyV1, RetryTimerScheduleRequestV1, StepPlanV1, apply_action_result,
    apply_manual_resolution, fire_retry_timer, replay, replay_ordered, schedule_retry_timer, start,
};
use penelope_ports::ManualReviewResolutionV1;
use std::num::{NonZeroU32, NonZeroU64};

fn identifier<T: TryFrom<&'static str>>(value: &'static str) -> T {
    match T::try_from(value) {
        Ok(identifier) => identifier,
        Err(_) => unreachable!(),
    }
}

fuzz_target!(|data: &[u8]| {
    if let Ok(envelope) = serde_json::from_slice::<LinearSagaEventEnvelopeV1>(data) {
        let _ = envelope.event.observed_outcome_kinds();
    }
    if let Ok(definition) = serde_json::from_slice::<LinearSagaDefinitionV1>(data) {
        let _ = definition.validate();
    }
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
    if let Ok(input) = serde_json::from_slice::<LinearSagaInputV1>(data) {
        let _ = input.to_event(&definition);
    }
    let tenant_id = identifier::<TenantId>("tnt_fuzz");
    let process_id = identifier::<ProcessId>("prc_fuzz");
    let action_id = identifier::<ActionId>("act_fuzz_start");
    let replay_events = [
        LinearSagaEventV1::started(
            &definition,
            identifier::<InputId>("inp_fuzz_start"),
            action_id.clone(),
        ),
        LinearSagaEventV1::ActionResultObserved {
            input_id: identifier::<InputId>("inp_fuzz_result"),
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
        action_id.clone(),
    ) else {
        return;
    };
    let _ = decision.planned_outcome_kinds();
    let start_event = LinearSagaEventV1::started(
        &definition,
        identifier::<InputId>("inp_fuzz_start"),
        action_id.clone(),
    );
    let outcome_ids = [
        identifier::<OutcomeId>("out_fuzz_input"),
        identifier::<OutcomeId>("out_fuzz_started"),
        identifier::<OutcomeId>("out_fuzz_planned"),
    ];
    let facts = start_event
        .observed_outcome_kinds()
        .iter()
        .chain(decision.planned_outcome_kinds())
        .zip(outcome_ids)
        .map(|(kind, outcome_id)| {
            ProcessOutcomeFactV1::new(
                outcome_id,
                if matches!(kind, penelope_domain::ProcessOutcomeKindV1::InputAccepted) {
                    CausationIdV1::Input(identifier("inp_fuzz_start"))
                } else {
                    CausationIdV1::Action(action_id.clone())
                },
                OutcomeActorV1::System,
                LogicalTimeV1(data.first().copied().map_or(0, u64::from)),
                *kind,
                ContentDigest([0; 32]),
            )
        })
        .collect::<Vec<_>>();
    let scope = ProcessScopeV1::new(
        tenant_id.clone(),
        process_id.clone(),
        definition.definition_id.clone(),
        definition.definition_version.clone(),
        definition.definition_digest,
    );
    let _ = decision.build_required_outcomes(
        &start_event,
        &scope,
        data.first().copied().map_or(0, u64::from),
        &facts,
    );

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
        let _ = decision.planned_outcome_kinds();
    }

    let backoff = match RetryBackoffV1::new(
        NonZeroU64::MIN,
        NonZeroU64::new(4).unwrap_or(NonZeroU64::MIN),
    ) {
        Ok(backoff) => backoff,
        Err(_) => return,
    };
    let timer_definition = LinearSagaDefinitionV1::new(
        identifier("def_timer"),
        identifier("dfv_one"),
        ContentDigest([88; 32]),
        vec![StepPlanV1::canonical_command(
            identifier("stp_timer"),
            ContentDigest([7; 32]),
            RetryPolicyV1::new(NonZeroU32::new(3).unwrap_or(NonZeroU32::MIN)).with_backoff(backoff),
        )],
    );
    let Ok(started) = start(
        &timer_definition,
        tenant_id.clone(),
        process_id.clone(),
        identifier("act_timer_first"),
    ) else {
        return;
    };
    let Some(active) = started.next_action.as_ref() else {
        return;
    };
    let now = LogicalTimeV1(data.first().copied().map_or(0, u64::from));
    let scheduled = schedule_retry_timer(
        &timer_definition,
        &started.projection,
        tenant_id.clone(),
        process_id.clone(),
        &RetryTimerScheduleRequestV1::new(
            ActionResultObservationV1::retryable_failure(active.action_id.clone()),
            Some(identifier("act_timer_fire")),
            now,
            RetryJitterSeedV1::from_digest(ContentDigest([data.first().copied().unwrap_or(0); 32])),
        ),
    );
    if let Ok(scheduled) = scheduled
        && let Some(timer) = scheduled.next_action
        && let Some(due_at) = scheduled.projection.retry_due_at
    {
        let _ = fire_retry_timer(
            &timer_definition,
            &scheduled.projection,
            tenant_id,
            process_id,
            &timer.action_id,
            due_at,
            identifier("act_timer_retry"),
        );
    }

    let review_definition = LinearSagaDefinitionV1::new(
        identifier("def_review"),
        identifier("dfv_one"),
        ContentDigest([77; 32]),
        vec![StepPlanV1::canonical_command(
            identifier("stp_review"),
            ContentDigest([6; 32]),
            RetryPolicyV1::no_retry(),
        )],
    );
    let Ok(review_started) = start(
        &review_definition,
        identifier("tnt_review"),
        identifier("prc_review"),
        identifier("act_review_first"),
    ) else {
        return;
    };
    let Some(review_action) = review_started.next_action.as_ref() else {
        return;
    };
    let Ok(escalated) = apply_action_result(
        &review_definition,
        &review_started.projection,
        identifier("tnt_review"),
        identifier("prc_review"),
        &ActionResultObservationV1::unknown(review_action.action_id.clone()),
        None,
    ) else {
        return;
    };
    let resolution = match data.first().map_or(0, |byte| byte % 4) {
        0 => ManualReviewResolutionV1::RetryAction,
        1 => ManualReviewResolutionV1::Compensate,
        2 => ManualReviewResolutionV1::Cancel,
        _ => ManualReviewResolutionV1::Escalate,
    };
    let next_action_id = match resolution {
        ManualReviewResolutionV1::RetryAction | ManualReviewResolutionV1::Compensate => {
            Some(identifier("act_review_resolution"))
        }
        ManualReviewResolutionV1::Cancel | ManualReviewResolutionV1::Escalate => None,
    };
    let _ = apply_manual_resolution(
        &review_definition,
        &escalated.projection,
        identifier("tnt_review"),
        identifier("prc_review"),
        resolution,
        next_action_id,
    );
});
