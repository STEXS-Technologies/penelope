#![no_main]

use libfuzzer_sys::fuzz_target;
use penelope_domain::{
    ActionId, CausationId, ContentDigest, DefinitionId, DefinitionVersion, InputId, LogicalTime,
    OutcomeActor, OutcomeId, ProcessActionKind, ProcessId, ProcessOutcomeFact, ProcessScope,
    TenantId,
};
use penelope_executor::engine::{
    ActionResult, ActionResultObservation, CompensationPlan, LinearSagaDefinition, LinearSagaEvent,
    LinearSagaEventEnvelope, LinearSagaInput, ProcessGraphDefinition, RetryBackoff,
    RetryJitterSeed, RetryPolicy, RetryTimerScheduleRequest, StepPlan, apply_action_result,
    apply_manual_resolution, fire_retry_timer, replay, replay_ordered, schedule_retry_timer, start,
};
use penelope_executor::graph::{
    GraphSagaEventEnvelope, GraphSagaInput, apply_graph_result, replay_graph, replay_graph_ordered,
    start_graph,
};
use penelope_ports::ManualReviewResolution;
use std::num::{NonZeroU32, NonZeroU64};

fn identifier<T: TryFrom<&'static str>>(value: &'static str) -> T {
    match T::try_from(value) {
        Ok(identifier) => identifier,
        Err(_) => unreachable!(),
    }
}

fuzz_target!(|data: &[u8]| {
    if let Ok(envelope) = serde_json::from_slice::<LinearSagaEventEnvelope>(data) {
        let _ = envelope.event.observed_outcome_kinds();
    }
    if let Ok(definition) = serde_json::from_slice::<LinearSagaDefinition>(data) {
        let _ = definition.validate();
    }
    if let Ok(graph) = serde_json::from_slice::<ProcessGraphDefinition>(data) {
        let _ = graph.validate();
        if let Ok(started) = start_graph(
            &graph,
            identifier::<TenantId>("tnt_graph_fuzz"),
            identifier::<ProcessId>("prc_graph_fuzz"),
            GraphSagaInput::Start {
                input_id: identifier::<InputId>("inp_graph_fuzz_start"),
                action_id: identifier::<ActionId>("act_graph_fuzz_start"),
            },
        ) {
            let _ = replay_graph(
                &graph,
                &identifier::<TenantId>("tnt_graph_fuzz"),
                &identifier::<ProcessId>("prc_graph_fuzz"),
                std::slice::from_ref(&started.event),
            );
            let _ = replay_graph_ordered(
                &graph,
                &identifier::<TenantId>("tnt_graph_fuzz"),
                &identifier::<ProcessId>("prc_graph_fuzz"),
                &[GraphSagaEventEnvelope {
                    sequence: 0,
                    event: started.event.clone(),
                }],
            );
            if let Some(action) = started.next_action {
                let _ = apply_graph_result(
                    &graph,
                    &started.projection,
                    GraphSagaInput::ActionResult {
                        input_id: identifier::<InputId>("inp_graph_fuzz_result"),
                        observation: ActionResultObservation::succeeded(action.action_id),
                        next_action_id: None,
                    },
                );
            }
        }
    }
    let step_count = data.first().map_or(0, |byte| usize::from(byte % 4));
    let definition = LinearSagaDefinition {
        definition_id: identifier::<DefinitionId>("def_fuzz"),
        definition_version: identifier::<DefinitionVersion>("dfv_one"),
        definition_digest: ContentDigest([99; 32]),
        steps: (0..step_count)
            .map(|index| StepPlan {
                step_id: match index {
                    0 => identifier("stp_zero"),
                    1 => identifier("stp_one"),
                    2 => identifier("stp_two"),
                    _ => identifier("stp_three"),
                },
                action_kind: ProcessActionKind::CanonicalCommand,
                payload_digest: ContentDigest([index as u8; 32]),
                retry_policy: RetryPolicy::no_retry(),
                compensation: Some(CompensationPlan::canonical_command(
                    ContentDigest([u8::MAX - index as u8; 32]),
                    RetryPolicy::no_retry(),
                )),
            })
            .collect(),
    };
    if let Ok(input) = serde_json::from_slice::<LinearSagaInput>(data) {
        let _ = input.to_event(&definition);
    }
    let tenant_id = identifier::<TenantId>("tnt_fuzz");
    let process_id = identifier::<ProcessId>("prc_fuzz");
    let action_id = identifier::<ActionId>("act_fuzz_start");
    let replay_events = [
        LinearSagaEvent::started(
            &definition,
            identifier::<InputId>("inp_fuzz_start"),
            action_id.clone(),
        ),
        LinearSagaEvent::ActionResultObserved {
            input_id: identifier::<InputId>("inp_fuzz_result"),
            observation: ActionResultObservation::succeeded(action_id.clone()),
            next_action_id: Some(identifier("act_fuzz_next")),
        },
    ];
    let _ = replay(&definition, &tenant_id, &process_id, &replay_events);
    let duplicate_input_events = [
        replay_events[0].clone(),
        LinearSagaEvent::ActionResultObserved {
            input_id: identifier::<InputId>("inp_fuzz_start"),
            observation: ActionResultObservation::succeeded(action_id.clone()),
            next_action_id: Some(identifier("act_fuzz_next")),
        },
    ];
    let _ = replay(
        &definition,
        &tenant_id,
        &process_id,
        &duplicate_input_events,
    );
    let ordered_replay_events = [
        LinearSagaEventEnvelope {
            sequence: 0,
            event: replay_events[0].clone(),
        },
        LinearSagaEventEnvelope {
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
    let start_event = LinearSagaEvent::started(
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
            ProcessOutcomeFact::new(
                outcome_id,
                if matches!(kind, penelope_domain::ProcessOutcomeKind::InputAccepted) {
                    CausationId::Input(identifier("inp_fuzz_start"))
                } else {
                    CausationId::Action(action_id.clone())
                },
                OutcomeActor::System,
                LogicalTime(data.first().copied().map_or(0, u64::from)),
                *kind,
                ContentDigest([0; 32]),
            )
        })
        .collect::<Vec<_>>();
    let scope = ProcessScope::new(
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
            0 => ActionResult::Succeeded,
            1 => ActionResult::RetryableFailure,
            2 => ActionResult::TerminalFailure,
            _ => ActionResult::Unknown,
        };
        let next_action_id = match result {
            ActionResult::Succeeded | ActionResult::RetryableFailure => {
                Some(identifier("act_fuzz_next"))
            }
            ActionResult::TerminalFailure | ActionResult::Unknown => None,
        };
        let Some(active_action) = decision.next_action.as_ref() else {
            return;
        };
        let observation = ActionResultObservation {
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

    let backoff = match RetryBackoff::new(
        NonZeroU64::MIN,
        NonZeroU64::new(4).unwrap_or(NonZeroU64::MIN),
    ) {
        Ok(backoff) => backoff,
        Err(_) => return,
    };
    let timer_definition = LinearSagaDefinition::new(
        identifier("def_timer"),
        identifier("dfv_one"),
        ContentDigest([88; 32]),
        vec![StepPlan::canonical_command(
            identifier("stp_timer"),
            ContentDigest([7; 32]),
            RetryPolicy::new(NonZeroU32::new(3).unwrap_or(NonZeroU32::MIN)).with_backoff(backoff),
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
    let now = LogicalTime(data.first().copied().map_or(0, u64::from));
    let scheduled = schedule_retry_timer(
        &timer_definition,
        &started.projection,
        tenant_id.clone(),
        process_id.clone(),
        &RetryTimerScheduleRequest::new(
            ActionResultObservation::retryable_failure(active.action_id.clone()),
            Some(identifier("act_timer_fire")),
            now,
            RetryJitterSeed::from_digest(ContentDigest([data.first().copied().unwrap_or(0); 32])),
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

    let review_definition = LinearSagaDefinition::new(
        identifier("def_review"),
        identifier("dfv_one"),
        ContentDigest([77; 32]),
        vec![StepPlan::canonical_command(
            identifier("stp_review"),
            ContentDigest([6; 32]),
            RetryPolicy::no_retry(),
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
        &ActionResultObservation::unknown(review_action.action_id.clone()),
        None,
    ) else {
        return;
    };
    let resolution = match data.first().map_or(0, |byte| byte % 4) {
        0 => ManualReviewResolution::RetryAction,
        1 => ManualReviewResolution::Compensate,
        2 => ManualReviewResolution::Cancel,
        _ => ManualReviewResolution::Escalate,
    };
    let next_action_id = match resolution {
        ManualReviewResolution::RetryAction | ManualReviewResolution::Compensate => {
            Some(identifier("act_review_resolution"))
        }
        ManualReviewResolution::Cancel | ManualReviewResolution::Escalate => None,
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
