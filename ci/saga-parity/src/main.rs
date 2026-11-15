//! The legacy branch compiles unchanged against 0.4; prepared uses only the facade.
#[cfg(feature = "prepared")]
use penelope::diagnostics::PreparedDecision;
use penelope::executor::{engine::*, graph::*};
use penelope::{ActionId, ContentDigest, ProcessId, TenantId};
use serde_json::json;
use std::{
    num::{NonZeroU32, NonZeroU64},
    str::FromStr,
};
fn id<T: FromStr>(value: &str) -> T {
    value.parse().ok().unwrap()
}

fn linear(
    definition: &LinearSagaDefinition,
    before: Option<&LinearSagaProjection>,
    tenant: &TenantId,
    process: &ProcessId,
    input: &LinearSagaInput,
) -> Result<(LinearSagaEvent, SagaDecision), EngineError> {
    #[cfg(feature = "prepared")]
    {
        let ready = penelope::saga::prepare_linear(definition, before, tenant, process, input)?;
        assert_eq!(ready.before(), before);
        assert_eq!(ready.candidate(), &ready.decision().projection);
        assert_eq!(ready.provenance().input, input);
        assert_eq!(ready.provenance().tenant, tenant);
        assert_eq!(ready.provenance().process, process);
        assert_eq!(
            ready.intents().collect::<Vec<_>>(),
            ready.decision().next_action.iter().collect::<Vec<_>>()
        );
        Ok(ready.into_parts())
    }
    #[cfg(not(feature = "prepared"))]
    {
        let decision = decide(definition, before, tenant.clone(), process.clone(), input)?;
        Ok((input.to_event(definition), decision))
    }
}
fn graph(
    definition: &ProcessGraphDefinition,
    before: Option<&GraphSagaProjection>,
    tenant: &TenantId,
    process: &ProcessId,
    input: GraphSagaInput,
) -> Result<GraphSagaDecision, GraphEngineError> {
    #[cfg(feature = "prepared")]
    {
        let ready = match before {
            None => penelope::saga::prepare_graph_start(
                definition,
                tenant.clone(),
                process.clone(),
                input,
            ),
            Some(previous) => penelope::saga::prepare_graph_result(definition, previous, input),
        }?;
        assert_eq!(ready.before(), before);
        assert_eq!(ready.candidate(), &ready.decision().projection);
        assert_eq!(ready.provenance(), &ready.decision().event);
        assert_eq!(
            ready.intents().collect::<Vec<_>>(),
            ready.decision().next_action.iter().collect::<Vec<_>>()
        );
        Ok(ready.into_decision())
    }
    #[cfg(not(feature = "prepared"))]
    match before {
        None => start_graph(definition, tenant.clone(), process.clone(), input),
        Some(previous) => apply_graph_result(definition, previous, input),
    }
}
fn policy(scenario: u32) -> RetryPolicy {
    let policy = RetryPolicy::new(NonZeroU32::new(1 + scenario % 3).unwrap());
    if scenario & 4 == 0 {
        return policy;
    }
    policy.with_backoff(
        RetryBackoff::new(NonZeroU64::new(2).unwrap(), NonZeroU64::new(8).unwrap()).unwrap(),
    )
}
fn definition(scenario: u32) -> LinearSagaDefinition {
    LinearSagaDefinition {
        definition_id: id("def_parity"),
        definition_version: id("dfv_one"),
        definition_digest: ContentDigest([7; 32]),
        steps: vec![
            StepPlan::canonical_command(id("stp_first"), ContentDigest([1; 32]), policy(scenario))
                .with_compensation(CompensationPlan {
                    action_kind: penelope::ProcessActionKind::CanonicalCommand,
                    payload_digest: ContentDigest([3; 32]),
                    retry_policy: RetryPolicy::no_retry(),
                }),
            StepPlan::canonical_command(id("stp_second"), ContentDigest([2; 32]), policy(scenario)),
        ],
    }
}
fn result(index: u32) -> ActionResult {
    match index % 4 {
        0 => ActionResult::Succeeded,
        1 => ActionResult::RetryableFailure,
        2 => ActionResult::TerminalFailure,
        _ => ActionResult::Unknown,
    }
}
fn linear_histories() {
    for scenario in 0_u32..100 {
        let mut definition = definition(scenario);
        if (80..96).contains(&scenario) {
            definition.steps.clear();
        }
        let tenant: TenantId = id("tnt_parity");
        let process: ProcessId = id("prc_parity");
        let mut previous: Option<SagaDecision> = None;
        let mut events = Vec::new();
        for round in 0..16 {
            let input_id = id(&format!("inp_{scenario}_{round}"));
            let fresh: ActionId = id(&format!("act_{scenario}_{round}"));
            let input = if let Some(before) = previous.as_ref() {
                let projection = &before.projection;
                let active = projection
                    .active_action_id
                    .clone()
                    .unwrap_or_else(|| id("act_absent"));
                let next = if round % 5 == 0 {
                    None
                } else {
                    Some(fresh.clone())
                };
                if projection.status == SagaStatus::Escalated {
                    LinearSagaInput::ManualResolution {
                        input_id,
                        resolution: match (scenario + round) % 4 {
                            0 => penelope::ManualReviewResolution::RetryAction,
                            1 => penelope::ManualReviewResolution::Compensate,
                            2 => penelope::ManualReviewResolution::Cancel,
                            _ => penelope::ManualReviewResolution::Escalate,
                        },
                        next_action_id: next,
                    }
                } else if let Some(due) = projection.retry_due_at {
                    LinearSagaInput::RetryTimerFired {
                        input_id,
                        timer_action_id: active,
                        fired_at: if round % 3 == 0 {
                            penelope::LogicalTime(0)
                        } else {
                            due
                        },
                        next_action_id: fresh,
                    }
                } else {
                    let observation = ActionResultObservation {
                        action_id: if round % 7 == 0 {
                            id("act_foreign")
                        } else {
                            active
                        },
                        result: if scenario >= 96 {
                            if round == 2 {
                                ActionResult::TerminalFailure
                            } else {
                                ActionResult::Succeeded
                            }
                        } else {
                            result(scenario + round)
                        },
                    };
                    if observation.result == ActionResult::RetryableFailure && scenario & 4 != 0 {
                        LinearSagaInput::RetryTimerScheduled {
                            input_id,
                            observation,
                            next_action_id: next,
                            observed_at: penelope::LogicalTime(u64::from(round)),
                            jitter_seed: RetryJitterSeed::from_digest(ContentDigest([6; 32])),
                            retry_deadline: None,
                        }
                    } else {
                        LinearSagaInput::ActionResult {
                            input_id,
                            observation,
                            next_action_id: next,
                        }
                    }
                }
            } else {
                LinearSagaInput::Start {
                    input_id,
                    action_id: fresh,
                }
            };
            match linear(
                &definition,
                previous.as_ref().map(|p| &p.projection),
                &tenant,
                &process,
                &input,
            ) {
                Ok((event, decision)) => {
                    println!(
                        "{}",
                        json!({"engine":"linear", "scenario":scenario, "round":round,
                        "event":event, "decision":decision})
                    );
                    events.push(event);
                    let replayed = replay(&definition, &tenant, &process, &events).unwrap();
                    assert_eq!(replayed, decision);
                    previous = Some(decision);
                }
                Err(error) => println!(
                    "{}",
                    json!({"engine":"linear", "scenario":scenario,
                    "round":round, "error":format!("{error:?}")})
                ),
            }
        }
    }
}
fn graph_histories() {
    for scenario in 0_u32..96 {
        let linear_definition = definition(scenario);
        let mut definition = ProcessGraphDefinition {
            definition_id: linear_definition.definition_id,
            definition_version: linear_definition.definition_version,
            definition_digest: linear_definition.definition_digest,
            steps: linear_definition.steps,
            entry_step_id: id("stp_first"),
            transitions: vec![
                GraphTransition {
                    from_step: id("stp_first"),
                    on: GraphTransitionOutcome::Succeeded,
                    to_step: Some(id("stp_second")),
                },
                GraphTransition {
                    from_step: id("stp_first"),
                    on: GraphTransitionOutcome::RetryableFailure,
                    to_step: Some(id("stp_first")),
                },
                GraphTransition {
                    from_step: id("stp_second"),
                    on: GraphTransitionOutcome::Succeeded,
                    to_step: None,
                },
            ],
            max_step_visits: NonZeroU32::new(1 + scenario % 5).unwrap(),
        };
        if scenario >= 80 {
            definition.transitions.push(GraphTransition {
                from_step: id("stp_foreign"),
                on: GraphTransitionOutcome::Succeeded,
                to_step: None,
            });
        }
        let tenant: TenantId = id("tnt_parity");
        let process: ProcessId = id("prc_parity");
        let mut previous: Option<GraphSagaDecision> = None;
        let mut events = Vec::new();
        for round in 0..16 {
            let input_id = id(&format!("inp_{scenario}_{round}"));
            let fresh = id(&format!("act_{scenario}_{round}"));
            let input = if let Some(before) = previous.as_ref() {
                GraphSagaInput::ActionResult {
                    input_id,
                    observation: ActionResultObservation {
                        action_id: if round % 7 == 0 {
                            id("act_foreign")
                        } else {
                            before
                                .projection
                                .active_action_id
                                .clone()
                                .unwrap_or_else(|| id("act_absent"))
                        },
                        result: result(scenario + round),
                    },
                    next_action_id: if round % 5 == 0
                        || result(scenario + round) == ActionResult::Unknown
                    {
                        None
                    } else {
                        Some(fresh)
                    },
                }
            } else {
                GraphSagaInput::Start {
                    input_id,
                    action_id: fresh,
                }
            };
            match graph(
                &definition,
                previous.as_ref().map(|p| &p.projection),
                &tenant,
                &process,
                input,
            ) {
                Ok(decision) => {
                    println!(
                        "{}",
                        json!({"engine":"graph", "scenario":scenario, "round":round,
                        "decision":decision})
                    );
                    events.push(decision.event.clone());
                    assert_eq!(
                        replay_graph(&definition, &tenant, &process, &events).unwrap(),
                        decision.projection
                    );
                    previous = Some(decision);
                }
                Err(error) => println!(
                    "{}",
                    json!({"engine":"graph", "scenario":scenario,
                    "round":round, "error":format!("{error:?}")})
                ),
            }
        }
    }
}
fn main() {
    linear_histories();
    graph_histories();
}
