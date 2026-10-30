//! Durable outcome plans for graph decisions, matching the linear engine's API.
use crate::{
    engine::{ActionResult, EngineError, SagaStatus},
    graph::{GraphSagaDecision, GraphSagaEvent},
};
use penelope_domain::{
    CausationId, InputId, ProcessOutcome, ProcessOutcomeFact, ProcessOutcomeKind, ProcessScope,
};

impl GraphSagaEvent {
    #[must_use]
    pub const fn input_id(&self) -> &InputId {
        match self {
            Self::Started { input_id, .. } | Self::ActionResultObserved { input_id, .. } => {
                input_id
            }
        }
    }

    #[must_use]
    pub const fn observed_outcome_kinds(&self) -> &'static [ProcessOutcomeKind] {
        use ProcessOutcomeKind::{
            ActionFailed, ActionOutcomeUnknown, ActionSucceeded, InputAccepted, Started,
        };
        match self {
            Self::Started { .. } => &[InputAccepted, Started],
            Self::ActionResultObserved { observation, .. } => match observation.result {
                ActionResult::Succeeded => &[InputAccepted, ActionSucceeded],
                ActionResult::RetryableFailure | ActionResult::TerminalFailure => {
                    &[InputAccepted, ActionFailed]
                }
                ActionResult::Unknown => &[InputAccepted, ActionOutcomeUnknown],
            },
        }
    }
}

impl GraphSagaDecision {
    #[must_use]
    pub const fn planned_outcome_kinds(&self) -> &'static [ProcessOutcomeKind] {
        if self.next_action.is_some() {
            return &[ProcessOutcomeKind::ActionPlanned];
        }
        match self.projection.status {
            SagaStatus::Completed => &[ProcessOutcomeKind::Completed],
            SagaStatus::Escalated => &[ProcessOutcomeKind::Escalated],
            SagaStatus::Cancelled => &[ProcessOutcomeKind::Cancelled],
            SagaStatus::Compensated => &[ProcessOutcomeKind::Compensated],
            SagaStatus::Running | SagaStatus::Compensating => &[],
        }
    }

    /// Builds the exact ordered outcome plan with caller-supplied immutable facts.
    /// No sequence, identifier, time or payload identity is invented by this API.
    /// # Errors
    /// Rejects scope, outcome kinds, input causation and sequence overflow.
    pub fn build_required_outcomes(
        &self,
        scope: &ProcessScope,
        first_sequence: u64,
        facts: &[ProcessOutcomeFact],
    ) -> Result<Vec<ProcessOutcome>, EngineError> {
        if scope != &self.projection.scope {
            return Err(EngineError::OutcomeScopeMismatch);
        }
        let expected = self
            .event
            .observed_outcome_kinds()
            .iter()
            .chain(self.planned_outcome_kinds())
            .copied();
        if !facts.iter().map(|fact| fact.kind).eq(expected) {
            return Err(EngineError::OutcomeKindPlanMismatch);
        }
        for fact in facts
            .iter()
            .filter(|fact| fact.kind == ProcessOutcomeKind::InputAccepted)
        {
            if fact.causation_id != CausationId::Input(self.event.input_id().clone()) {
                return Err(EngineError::OutcomeInputCausationMismatch);
            }
        }
        facts
            .iter()
            .enumerate()
            .map(|(index, fact)| {
                let offset = u64::try_from(index)
                    .map_err(|_overflow| EngineError::OutcomeSequenceOverflow)?;
                let sequence = first_sequence
                    .checked_add(offset)
                    .ok_or(EngineError::OutcomeSequenceOverflow)?;
                Ok(ProcessOutcome::new(scope.clone(), sequence, fact.clone()))
            })
            .collect()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::{
        engine::{
            GraphTransition, GraphTransitionOutcome, ProcessGraphDefinition, RetryPolicy, StepPlan,
        },
        graph::{GraphSagaInput, start_graph},
    };
    use penelope_domain::{ContentDigest, LogicalTime, OutcomeActor, ProcessActionKind};
    use std::num::NonZeroU32;
    fn id<T: std::str::FromStr>(text: &str) -> T {
        text.parse().ok().unwrap()
    }
    #[test]
    fn graph_outcome_plan_checks_order_scope_causation_and_sequence() {
        let definition = ProcessGraphDefinition {
            definition_id: id("def_graph"),
            definition_version: id("dfv_one"),
            definition_digest: ContentDigest([1; 32]),
            steps: vec![StepPlan {
                step_id: id("stp_work"),
                action_kind: ProcessActionKind::ExternalEffect,
                payload_digest: ContentDigest([2; 32]),
                retry_policy: RetryPolicy::no_retry(),
                compensation: None,
            }],
            entry_step_id: id("stp_work"),
            transitions: vec![GraphTransition {
                from_step: id("stp_work"),
                on: GraphTransitionOutcome::Succeeded,
                to_step: None,
            }],
            max_step_visits: NonZeroU32::MIN,
        };
        let decision = start_graph(
            &definition,
            id("tnt_graph"),
            id("prc_graph"),
            GraphSagaInput::Start {
                input_id: id("inp_start"),
                action_id: id("act_work"),
            },
        )
        .unwrap();
        let facts: Vec<_> = decision
            .event
            .observed_outcome_kinds()
            .iter()
            .chain(decision.planned_outcome_kinds())
            .enumerate()
            .map(|(index, kind)| {
                ProcessOutcomeFact::new(
                    id(&format!("out_graph_{index}")),
                    CausationId::Input(id("inp_start")),
                    OutcomeActor::System,
                    LogicalTime(0),
                    *kind,
                    ContentDigest([3; 32]),
                )
            })
            .collect();
        let scope = &decision.projection.scope;
        let outcomes = decision.build_required_outcomes(scope, 0, &facts).unwrap();
        assert_eq!(outcomes.len(), 3);
        assert_eq!(outcomes.last().unwrap().sequence, 2);
        let mut reversed = facts.clone();
        reversed.reverse();
        assert_eq!(
            decision.build_required_outcomes(scope, 0, &reversed),
            Err(EngineError::OutcomeKindPlanMismatch)
        );
        let mut foreign = scope.clone();
        foreign.tenant_id = id("tnt_other");
        assert_eq!(
            decision.build_required_outcomes(&foreign, 0, &facts),
            Err(EngineError::OutcomeScopeMismatch)
        );
        assert_eq!(
            decision.build_required_outcomes(scope, u64::MAX, &facts),
            Err(EngineError::OutcomeSequenceOverflow)
        );
        let changed: Vec<_> = facts
            .into_iter()
            .map(|mut fact| {
                if fact.kind == ProcessOutcomeKind::InputAccepted {
                    fact.causation_id = CausationId::Input(id("inp_other"));
                }
                fact
            })
            .collect();
        assert_eq!(
            decision.build_required_outcomes(scope, 0, &changed),
            Err(EngineError::OutcomeInputCausationMismatch)
        );
    }
}
