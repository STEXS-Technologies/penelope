//! Immutable wire fixtures. A change here is a protocol change, not a
//! formatting preference: add a new typed value instead of reinterpreting it.

#![allow(clippy::expect_used, clippy::needless_pass_by_value)]

use core::fmt::Debug;

use penelope_domain::{
    ActionId, CanonicalCommand, CanonicalCommitId, CanonicalEvent, CanonicalEventId, CausationId,
    ContentDigest, DefinitionId, DefinitionVersion, DomainError, InputId, LogicalTime,
    ManualReview, OperationId, OutcomeActor, OutcomeId, ProcessAction, ProcessActionKind,
    ProcessDefinition, ProcessId, ProcessInput, ProcessInputEnvelope, ProcessInputKind,
    ProcessOutcome, ProcessOutcomeFact, ProcessOutcomeKind, ProcessScope, ResourceId, ReviewId,
    StepId, TenantId,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

fn id<T: TryFrom<&'static str>>(value: &'static str) -> T {
    T::try_from(value)
        .ok()
        .expect("fixture identifier is valid")
}

fn assert_fixture<T>(
    fixture: &str,
    expected: T,
    validate: impl FnOnce(&T) -> Result<(), DomainError>,
) where
    T: DeserializeOwned + Serialize + PartialEq + Debug,
{
    let decoded: T = serde_json::from_str(fixture).expect("fixture deserializes");
    validate(&decoded).expect("fixture satisfies its typed invariants");
    assert_eq!(
        decoded, expected,
        "fixture decodes to the expected typed DTO"
    );

    let fixture_json: serde_json::Value = serde_json::from_str(fixture).expect("fixture is JSON");
    assert_eq!(
        serde_json::to_value(&expected).expect("DTO serializes"),
        fixture_json,
        "DTO serialization preserves the immutable wire fixture"
    );
}

#[test]
fn wire_fixtures_are_immutable_and_typed() {
    let scope = ProcessScope::new(
        id::<TenantId>("tnt_market"),
        id::<ProcessId>("prc_trade"),
        id::<DefinitionId>("def_trade"),
        id::<DefinitionVersion>("dfv_one"),
        ContentDigest([1; 32]),
    );
    let input = ProcessInput::new(
        id("tnt_market"),
        id("prc_trade"),
        id::<InputId>("inp_start"),
        ProcessInputKind::CanonicalEvent,
        ContentDigest([2; 32]),
    );

    assert_fixture(
        include_str!("fixtures/current/process-definition.json"),
        ProcessDefinition::new(
            id("def_trade"),
            id("dfv_one"),
            ContentDigest([1; 32]),
            vec![id::<StepId>("stp_lock"), id("stp_settle")],
        )
        .expect("fixture definition is valid"),
        ProcessDefinition::validate,
    );
    assert_fixture(
        include_str!("fixtures/current/process-input.json"),
        input.clone(),
        ProcessInput::validate,
    );
    assert_fixture(
        include_str!("fixtures/current/process-input-envelope.json"),
        ProcessInputEnvelope::new(input),
        ProcessInputEnvelope::validate,
    );
    assert_fixture(
        include_str!("fixtures/current/process-outcome.json"),
        ProcessOutcome::new(
            scope.clone(),
            0,
            ProcessOutcomeFact::new(
                id::<OutcomeId>("out_started"),
                CausationId::Input(id::<InputId>("inp_start")),
                OutcomeActor::System,
                LogicalTime(7),
                ProcessOutcomeKind::Started,
                ContentDigest([3; 32]),
            ),
        ),
        ProcessOutcome::validate,
    );
    assert_fixture(
        include_str!("fixtures/current/process-action.json"),
        ProcessAction::new(
            scope.clone(),
            id::<ActionId>("act_lock"),
            id::<StepId>("stp_lock"),
            0,
            ProcessActionKind::CanonicalCommand,
            ContentDigest([4; 32]),
        ),
        ProcessAction::validate,
    );
    assert_fixture(
        include_str!("fixtures/current/canonical-command.json"),
        CanonicalCommand::new(
            id("tnt_market"),
            id::<ActionId>("act_lock"),
            id::<OperationId>("op_lock"),
            vec![id::<ResourceId>("res_asset_a"), id("res_asset_b")],
            ContentDigest([5; 32]),
        )
        .expect("fixture command is valid"),
        CanonicalCommand::validate,
    );
    assert_fixture(
        include_str!("fixtures/current/canonical-event.json"),
        CanonicalEvent::new(
            id("tnt_market"),
            id::<CanonicalEventId>("cev_lock"),
            id::<ActionId>("act_lock"),
            id::<CanonicalCommitId>("cmt_lock"),
            9,
            id::<OperationId>("op_lock"),
            vec![id::<ResourceId>("res_asset_a"), id("res_asset_b")],
            ContentDigest([6; 32]),
        )
        .expect("fixture event is valid"),
        CanonicalEvent::validate,
    );
    assert_fixture(
        include_str!("fixtures/current/manual-review.json"),
        ManualReview::new(
            scope,
            id::<ReviewId>("rev_trade"),
            10,
            Some(LogicalTime(42)),
            ContentDigest([7; 32]),
        ),
        ManualReview::validate,
    );
}
