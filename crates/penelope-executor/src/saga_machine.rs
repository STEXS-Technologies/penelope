//! Saga preparation through the existing decision engines and commit protocols.
use crate::{engine::*, graph::*};
use penelope_core::machine::PreparedDecision;
use penelope_domain::{ProcessAction, ProcessId, TenantId};

/// Exact borrowed inputs to a linear decision. Tenant/process are caller-supplied:
/// the legacy linear projection does not itself record those coordinates.
/// No durable sequence, machine revision or authorization receipt is invented.
pub struct LinearSagaSource<'input> {
    pub definition: &'input LinearSagaDefinition,
    pub tenant: &'input TenantId,
    pub process: &'input ProcessId,
    pub input: &'input LinearSagaInput,
}

/// Linear candidate evaluated by the original engine. No commit or dispatch occurs.
pub struct PreparedLinearSaga<'input> {
    before: Option<&'input LinearSagaProjection>,
    decision: SagaDecision,
    source: LinearSagaSource<'input>,
}
impl PreparedLinearSaga<'_> {
    /// Original decision, for the existing exact outcome-plan/atomic commit path.
    pub const fn decision(&self) -> &SagaDecision {
        &self.decision
    }

    /// Transfers the exact legacy replay event and decision to durable admission.
    /// Neither value is a commit receipt or permission to dispatch an action.
    pub fn into_parts(self) -> (LinearSagaEvent, SagaDecision) {
        (
            self.source.input.to_event(self.source.definition),
            self.decision,
        )
    }
}
impl<'input> PreparedDecision for PreparedLinearSaga<'input> {
    type State = LinearSagaProjection;
    type Provenance = LinearSagaSource<'input>;
    type Intent = ProcessAction;
    fn before(&self) -> Option<&Self::State> {
        self.before
    }
    fn candidate(&self) -> &Self::State {
        &self.decision.projection
    }
    fn provenance(&self) -> &Self::Provenance {
        &self.source
    }
    fn intents(&self) -> impl Iterator<Item = &ProcessAction> {
        self.decision.next_action.iter()
    }
}

/// Prepares a linear saga using exactly the existing `decide` validation path.
/// Caller-supplied tenant/process retain the legacy external scope trust boundary.
/// # Errors
/// Returns the original EngineError, without translating refusal or ambiguity.
pub fn prepare_linear<'input>(
    definition: &'input LinearSagaDefinition,
    before: Option<&'input LinearSagaProjection>,
    tenant: &'input TenantId,
    process: &'input ProcessId,
    input: &'input LinearSagaInput,
) -> Result<PreparedLinearSaga<'input>, EngineError> {
    let decision = decide(definition, before, tenant.clone(), process.clone(), input)?;
    Ok(PreparedLinearSaga {
        before,
        decision,
        source: LinearSagaSource {
            definition,
            tenant,
            process,
            input,
        },
    })
}

/// Graph candidate with the original scoped projection, event and planned action.
/// Its event is proposed provenance; scope remains in the graph projection.
pub struct PreparedGraphSaga<'projection> {
    before: Option<&'projection GraphSagaProjection>,
    decision: GraphSagaDecision,
}
impl PreparedGraphSaga<'_> {
    pub const fn decision(&self) -> &GraphSagaDecision {
        &self.decision
    }
    /// Transfers the unchanged decision to existing graph outcome/commit builders.
    pub fn into_decision(self) -> GraphSagaDecision {
        self.decision
    }
}
impl PreparedDecision for PreparedGraphSaga<'_> {
    type State = GraphSagaProjection;
    type Provenance = GraphSagaEvent;
    type Intent = ProcessAction;
    fn before(&self) -> Option<&Self::State> {
        self.before
    }
    fn candidate(&self) -> &Self::State {
        &self.decision.projection
    }
    fn provenance(&self) -> &GraphSagaEvent {
        &self.decision.event
    }
    fn intents(&self) -> impl Iterator<Item = &ProcessAction> {
        self.decision.next_action.iter()
    }
}

/// Prepares the original graph start; no second graph interpreter or state owner.
/// # Errors
/// Returns the original start_graph error and validation precedence.
pub fn prepare_graph_start<'definition>(
    definition: &'definition ProcessGraphDefinition,
    tenant: TenantId,
    process: ProcessId,
    input: GraphSagaInput,
) -> Result<PreparedGraphSaga<'definition>, GraphEngineError> {
    Ok(PreparedGraphSaga {
        before: None,
        decision: start_graph(definition, tenant, process, input)?,
    })
}

/// Prepares the original graph result, retaining its exact pinned scope and event.
/// # Errors
/// Returns the original apply_graph_result error and validation precedence.
pub fn prepare_graph_result<'projection>(
    definition: &ProcessGraphDefinition,
    before: &'projection GraphSagaProjection,
    input: GraphSagaInput,
) -> Result<PreparedGraphSaga<'projection>, GraphEngineError> {
    Ok(PreparedGraphSaga {
        before: Some(before),
        decision: apply_graph_result(definition, before, input)?,
    })
}
