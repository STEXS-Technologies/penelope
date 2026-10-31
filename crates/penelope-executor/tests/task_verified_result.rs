#![allow(clippy::unwrap_used)]
#[path = "support/tasks.rs"]
mod support;
use penelope_core::task::*;
use penelope_domain::{ContentDigest, ProcessAction, ProcessActionKind, ProcessScope};
use penelope_ports::{
    EffectDispatchRequest, ExternalEffectEvidence, ExternalEffectState,
    tasks::{
        TaskEffectBinding, TaskEffectEvidenceVerifier, TaskEffectVerificationError as Error,
        TaskOutputClassifier, VerifiedTaskEffectOutcome,
    },
};

struct Classifier(ExternalEffectState);
impl TaskOutputClassifier for Classifier {
    fn identity(&self) -> Digest {
        Digest([7; 32])
    }
    fn classify(
        &self,
        _binding: &TaskEffectBinding,
        output: BlobRef,
        bytes: &[u8],
    ) -> Result<ExternalEffectState, Error> {
        if output.schema != 1 || bytes != b"valid" {
            return Err(Error::InvalidOutput);
        }
        Ok(self.0)
    }
}
struct Verifier(ExternalEffectEvidence);
impl TaskEffectEvidenceVerifier for Verifier {
    fn verify(
        &self,
        _binding: &TaskEffectBinding,
        _generation: TaskGeneration,
        _output: BlobRef,
        _bytes: &[u8],
    ) -> Result<ExternalEffectEvidence, Error> {
        Ok(self.0.clone())
    }
}
fn completed(effects: EffectContract) -> (TaskEffectBinding, TaskProjection) {
    let scope = ProcessScope::new(
        "tnt_demo".try_into().unwrap(),
        "prc_demo".try_into().unwrap(),
        "def_demo".try_into().unwrap(),
        "dfv_one".try_into().unwrap(),
        ContentDigest([1; 32]),
    );
    let request = EffectDispatchRequest::new(ProcessAction::new(
        scope,
        "act_compute".try_into().unwrap(),
        "stp_compute".try_into().unwrap(),
        0,
        ProcessActionKind::ExternalEffect,
        ContentDigest::sha256(b"input"),
    ));
    let definition = support::definition(RecoveryMode::RestartFromCheckpoint, effects);
    let binding = TaskEffectBinding::new(request, definition).unwrap();
    let mut projection = TaskProjection::new(binding.scope(), definition).unwrap();
    let intent = projection
        .propose(
            TaskExecutionEvent::IntentCommitted {
                definition,
                input: BlobRef {
                    digest: Digest::of(b"input"),
                    bytes: 5,
                    schema: 1,
                },
            },
            TaskGeneration::ZERO,
            TaskTick::ZERO,
        )
        .unwrap();
    projection.apply(&intent).unwrap();
    let start = projection.next(TaskTick::new(1)).unwrap().unwrap();
    projection.apply(&start).unwrap();
    let result = projection
        .propose(
            TaskExecutionEvent::Completed {
                output: BlobRef {
                    digest: Digest::of(b"valid"),
                    bytes: 5,
                    schema: 1,
                },
            },
            projection.generation(),
            TaskTick::new(2),
        )
        .unwrap();
    projection.apply(&result).unwrap();
    (binding, projection)
}
#[test]
fn pure_completion_requires_exact_output_and_resolved_application_meaning() {
    let (binding, projection) = completed(EffectContract::Pure);
    let success = Classifier(ExternalEffectState::Succeeded);
    let verify = |bytes: &[u8], generation, classifier: &Classifier| {
        VerifiedTaskEffectOutcome::verify(
            &binding,
            &projection,
            generation,
            bytes,
            classifier,
            None,
        )
    };
    let outcome = verify(b"valid", projection.generation(), &success).unwrap();
    assert_eq!(outcome.evidence().state, ExternalEffectState::Succeeded);
    assert_eq!(outcome.binding(), &binding);
    assert_eq!(outcome.output(), projection.output().unwrap());
    assert_eq!(outcome.classifier(), Digest([7; 32]));
    assert_eq!(outcome.generation(), projection.generation());
    assert_eq!(
        verify(b"other", projection.generation(), &success),
        Err(Error::CorruptOutput)
    );
    assert_eq!(
        verify(b"valid", TaskGeneration::ZERO, &success),
        Err(Error::BindingMismatch)
    );
    assert_eq!(
        verify(
            b"valid",
            projection.generation(),
            &Classifier(ExternalEffectState::Unknown)
        ),
        Err(Error::Unresolved)
    );
    assert_eq!(
        verify(
            b"valid",
            projection.generation(),
            &Classifier(ExternalEffectState::KnownFailure)
        )
        .unwrap()
        .evidence()
        .state,
        ExternalEffectState::KnownFailure
    );
    let empty = TaskProjection::new(binding.scope(), binding.definition()).unwrap();
    assert_eq!(
        VerifiedTaskEffectOutcome::verify(
            &binding,
            &empty,
            TaskGeneration::ZERO,
            b"valid",
            &success,
            None
        ),
        Err(Error::NotCompleted)
    );
}
#[test]
fn effectful_completion_requires_matching_independent_evidence() {
    let (binding, projection) = completed(EffectContract::ReconcileRequired);
    let success = Classifier(ExternalEffectState::Succeeded);
    let verify = |verifier: Option<&dyn TaskEffectEvidenceVerifier>| {
        VerifiedTaskEffectOutcome::verify(
            &binding,
            &projection,
            projection.generation(),
            b"valid",
            &success,
            verifier,
        )
    };
    assert_eq!(verify(None), Err(Error::MissingEvidence));
    let mut evidence = ExternalEffectEvidence {
        action_id: binding.request().action.action_id.clone(),
        effect_key: binding.request().effect_key.clone(),
        external_reference_id: None,
        state: ExternalEffectState::Succeeded,
    };
    assert_eq!(
        verify(Some(&Verifier(evidence.clone())))
            .unwrap()
            .evidence(),
        &evidence
    );
    evidence.state = ExternalEffectState::Unknown;
    assert_eq!(
        verify(Some(&Verifier(evidence.clone()))),
        Err(Error::Unresolved)
    );
    evidence.state = ExternalEffectState::KnownFailure;
    assert_eq!(
        verify(Some(&Verifier(evidence.clone()))),
        Err(Error::EvidenceMismatch)
    );
    evidence.state = ExternalEffectState::Succeeded;
    evidence.effect_key.attempt = 1;
    assert_eq!(
        verify(Some(&Verifier(evidence))),
        Err(Error::EvidenceMismatch)
    );
}

#[test]
fn result_relay_maps_to_both_engines_with_one_delivery_identity() {
    use penelope_domain::ProcessInputKind;
    use penelope_executor::{
        composition::TaskResultRelay,
        engine::{ActionResult, LinearSagaInput},
        graph::GraphSagaInput,
    };
    for state in [
        ExternalEffectState::Succeeded,
        ExternalEffectState::KnownFailure,
    ] {
        let (binding, projection) = completed(EffectContract::Pure);
        let verified = VerifiedTaskEffectOutcome::verify(
            &binding,
            &projection,
            projection.generation(),
            b"valid",
            &Classifier(state),
            None,
        )
        .unwrap();
        let relay = TaskResultRelay::new(verified.clone()).unwrap();
        let repeated = TaskResultRelay::new(verified).unwrap();
        assert_eq!(relay.input(), repeated.input());
        assert_eq!(relay.input().kind, ProcessInputKind::TaskEffectResult);
        assert_eq!(relay.input().tenant_id, binding.request().action.tenant_id);
        assert_eq!(
            relay.observation().result,
            if state == ExternalEffectState::Succeeded {
                ActionResult::Succeeded
            } else {
                ActionResult::TerminalFailure
            }
        );
        assert_eq!(
            relay.linear_input(None),
            LinearSagaInput::ActionResult {
                input_id: relay.input().input_id.clone(),
                observation: relay.observation(),
                next_action_id: None,
            }
        );
        assert_eq!(
            relay.graph_input(None),
            GraphSagaInput::ActionResult {
                input_id: relay.input().input_id.clone(),
                observation: relay.observation(),
                next_action_id: None,
            }
        );
    }
}
