use penelope_domain::*;
use penelope_executor::{engine::*, graph::*};
use std::{error::Error as StdError, num::NonZeroU32, str::FromStr};
mod storage;
use storage::Storage;
pub type Error = Box<dyn StdError + Send + Sync>;
pub fn id<T: FromStr>(value: &str) -> Result<T, Error>
where
    T::Err: StdError + Send + Sync + 'static,
{
    Ok(value.parse()?)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Case {
    Linear,
    Graph,
    Retry,
    Replay,
    Durable,
    DurableReplay,
}
impl Case {
    pub const ALL: [Self; 6] = [
        Self::Linear,
        Self::Graph,
        Self::Retry,
        Self::Replay,
        Self::Durable,
        Self::DurableReplay,
    ];
    pub const fn name(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Graph => "graph",
            Self::Retry => "retry",
            Self::Replay => "replay",
            Self::Durable => "durable",
            Self::DurableReplay => "durable_replay",
        }
    }
    pub fn parse(value: &str) -> Result<Self, Error> {
        Self::ALL
            .into_iter()
            .find(|case| case.name() == value)
            .ok_or_else(|| "unknown workload case".into())
    }
    pub const fn calls(self) -> u32 {
        match self {
            Self::Graph => 4,
            Self::Retry => 9,
            Self::Linear | Self::Replay | Self::Durable | Self::DurableReplay => 8,
        }
    }
    const fn durable(self) -> bool {
        matches!(self, Self::Durable | Self::DurableReplay)
    }
}
#[inline(never)]
pub fn kernel(mut value: u64, work: u32) -> u64 {
    for _ in 0..work {
        value = value
            .wrapping_mul(6_364_136_223_846_793_005)
            .rotate_left(17)
            .wrapping_add(1_442_695_040_888_963_407);
    }
    value
}
pub struct Fixture {
    linear: LinearSagaDefinition,
    graph: ProcessGraphDefinition,
    storage: Option<Storage>,
}
impl Fixture {
    pub fn new(case: Case) -> Result<Self, Error> {
        let steps = (0..8)
            .map(|index| {
                Ok(StepPlan::canonical_command(
                    id(&format!("stp_{index}"))?,
                    ContentDigest([7; 32]),
                    RetryPolicy {
                        max_attempts: NonZeroU32::new(2).ok_or("attempt bound")?,
                        backoff: None,
                    },
                ))
            })
            .collect::<Result<Vec<_>, Error>>()?;
        let linear = LinearSagaDefinition::new(
            id("def_workload")?,
            id("dfv_one")?,
            ContentDigest([1; 32]),
            steps,
        );
        let graph_steps = ["stp_entry", "stp_yes", "stp_no", "stp_join", "stp_finish"]
            .into_iter()
            .map(|name| {
                Ok(StepPlan::canonical_command(
                    id(name)?,
                    ContentDigest([7; 32]),
                    RetryPolicy::no_retry(),
                ))
            })
            .collect::<Result<Vec<_>, Error>>()?;
        let transitions = [
            (
                "stp_entry",
                GraphTransitionOutcome::Succeeded,
                Some("stp_yes"),
            ),
            (
                "stp_entry",
                GraphTransitionOutcome::TerminalFailure,
                Some("stp_no"),
            ),
            (
                "stp_yes",
                GraphTransitionOutcome::Succeeded,
                Some("stp_join"),
            ),
            (
                "stp_no",
                GraphTransitionOutcome::Succeeded,
                Some("stp_join"),
            ),
            (
                "stp_join",
                GraphTransitionOutcome::Succeeded,
                Some("stp_finish"),
            ),
            ("stp_finish", GraphTransitionOutcome::Succeeded, None),
        ]
        .into_iter()
        .map(|(from, on, to)| {
            Ok(GraphTransition {
                from_step: id(from)?,
                on,
                to_step: to.map(id).transpose()?,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
        let graph = ProcessGraphDefinition {
            definition_id: id("def_graph_workload")?,
            definition_version: id("dfv_one")?,
            definition_digest: ContentDigest([2; 32]),
            steps: graph_steps,
            entry_step_id: id("stp_entry")?,
            transitions,
            max_step_visits: NonZeroU32::new(4).ok_or("visit bound")?,
        };
        linear.validate()?;
        graph.validate()?;
        Ok(Self {
            linear,
            graph,
            storage: if case.durable() {
                Some(Storage::new()?)
            } else {
                None
            },
        })
    }
    pub fn run(&mut self, case: Case, seed: u64, work: u32) -> Result<u64, Error> {
        if case == Case::Graph {
            return self.graph(seed, work);
        }
        let tenant = id::<TenantId>("tnt_workload")?;
        let process = id::<ProcessId>(&format!("prc_workload_{seed}"))?;
        let scope = ProcessScope::new(
            tenant.clone(),
            process.clone(),
            self.linear.definition_id.clone(),
            self.linear.definition_version.clone(),
            self.linear.definition_digest,
        );
        let start = LinearSagaInput::Start {
            input_id: id("inp_start")?,
            action_id: id("act_0")?,
        };
        let mut decision = decide(&self.linear, None, tenant.clone(), process.clone(), &start)?;
        let mut events = vec![start.to_event(&self.linear)];
        if let Some(storage) = &mut self.storage {
            storage.commit(
                &scope,
                &decision,
                events.first().ok_or("missing start event")?,
                None,
            )?;
        }
        let mut value = seed;
        let mut step = 0;
        let mut retried = false;
        for attempt in 0..case.calls() {
            let action = decision
                .next_action
                .as_ref()
                .ok_or("missing planned action")?
                .clone();
            if let Some(storage) = &mut self.storage {
                storage.claim(&scope, &action)?;
            }
            value = kernel(std::hint::black_box(value), std::hint::black_box(work));
            let fail = case == Case::Retry && !retried;
            let input = LinearSagaInput::ActionResult {
                input_id: id(&format!("inp_result_{attempt}"))?,
                observation: ActionResultObservation {
                    action_id: action.action_id.clone(),
                    result: if fail {
                        ActionResult::RetryableFailure
                    } else {
                        ActionResult::Succeeded
                    },
                },
                next_action_id: if step < 7 || fail {
                    Some(id(&format!("act_{}", attempt + 1))?)
                } else {
                    None
                },
            };
            decision = decide(
                &self.linear,
                Some(&decision.projection),
                tenant.clone(),
                process.clone(),
                &input,
            )?;
            let event = input.to_event(&self.linear);
            if let Some(storage) = &mut self.storage {
                storage.result(&scope, &action, value)?;
                storage.commit(&scope, &decision, &event, Some(value))?;
                storage.acknowledge()?;
            }
            events.push(event);
            if fail {
                retried = true;
            } else {
                step += 1;
            }
            if attempt == 3 && matches!(case, Case::Replay | Case::DurableReplay) {
                let recorded = if let Some(storage) = &mut self.storage {
                    let expected_value = value;
                    value = 0;
                    std::hint::black_box(value);
                    storage.reopen()?;
                    let (recorded, restored) = storage.events(&scope)?;
                    value = restored.ok_or("missing durable business state")?;
                    if value != expected_value {
                        return Err("durable business recovery differs".into());
                    }
                    recorded
                } else {
                    events.clone()
                };
                let recovered = replay(&self.linear, &tenant, &process, &recorded)?;
                if recovered.projection != decision.projection
                    || recovered.next_action != decision.next_action
                {
                    return Err("recovery projection differs".into());
                }
                decision = recovered;
            }
        }
        if step != 8
            || decision.projection.status != SagaStatus::Completed
            || decision.next_action.is_some()
        {
            return Err("workflow did not complete".into());
        }
        if let Some(storage) = &mut self.storage {
            let (recorded, restored) = storage.events(&scope)?;
            if restored != Some(value) {
                return Err("durable terminal business result differs".into());
            }
            let recovered = replay(&self.linear, &tenant, &process, &recorded)?;
            if recovered.projection != decision.projection {
                return Err("durable terminal replay differs".into());
            }
            storage.check_finished(&scope)?;
        }
        Ok(value)
    }
    fn graph(&self, seed: u64, work: u32) -> Result<u64, Error> {
        let mut decision = start_graph(
            &self.graph,
            id("tnt_workload")?,
            id("prc_graph")?,
            GraphSagaInput::Start {
                input_id: id("inp_start")?,
                action_id: id("act_0")?,
            },
        )?;
        let mut value = seed;
        for step in 0..4 {
            let action = decision
                .next_action
                .as_ref()
                .ok_or("missing graph action")?;
            let expected_step = match step {
                0 => "stp_entry",
                1 => {
                    if seed % 2 == 1 {
                        "stp_no"
                    } else {
                        "stp_yes"
                    }
                }
                2 => "stp_join",
                3 => "stp_finish",
                _ => return Err("unexpected graph step".into()),
            };
            if action.step_id.as_str() != expected_step {
                return Err("graph branch differs".into());
            }
            value = kernel(std::hint::black_box(value), std::hint::black_box(work));
            decision = apply_graph_result(
                &self.graph,
                &decision.projection,
                GraphSagaInput::ActionResult {
                    input_id: id(&format!("inp_result_{step}"))?,
                    observation: ActionResultObservation {
                        action_id: action.action_id.clone(),
                        result: if step == 0 && seed % 2 == 1 {
                            ActionResult::TerminalFailure
                        } else {
                            ActionResult::Succeeded
                        },
                    },
                    next_action_id: if step < 3 {
                        Some(id(&format!("act_{}", step + 1))?)
                    } else {
                        None
                    },
                },
            )?;
        }
        if decision.projection.status != SagaStatus::Completed || decision.next_action.is_some() {
            return Err("graph did not complete".into());
        }
        Ok(value)
    }
}
