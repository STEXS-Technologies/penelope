//! Immutable v1 wire fixtures. A change here is a protocol change, not a
//! formatting preference: add a new versioned DTO instead of reinterpreting it.

#![allow(clippy::expect_used, clippy::needless_pass_by_value)]

use core::fmt::Debug;

use penelope_domain::{
    ActionId, CanonicalCommandDtoV1, CanonicalCommitId, CanonicalEventDtoV1, CanonicalEventId,
    CausationIdV1, ContentDigest, DefinitionId, DefinitionVersion, DomainError, InputId,
    LogicalTimeV1, ManualReviewDtoV1, OperationId, OutcomeActorV1, OutcomeId, ProcessActionDtoV1,
    ProcessActionKindV1, ProcessDefinitionDtoV1, ProcessId, ProcessInputDtoV1,
    ProcessInputEnvelopeV1, ProcessInputKindV1, ProcessOutcomeDtoV1, ProcessOutcomeFactV1,
    ProcessOutcomeKindV1, ProcessScopeV1, ResourceId, ReviewId, StepId, TenantId,
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
    validate(&decoded).expect("fixture satisfies its schema invariant");
    assert_eq!(
        decoded, expected,
        "fixture decodes to the expected typed DTO"
    );

    let fixture_json: serde_json::Value = serde_json::from_str(fixture).expect("fixture is JSON");
    assert_eq!(
        serde_json::to_value(&expected).expect("DTO serializes"),
        fixture_json,
        "DTO serialization preserves the immutable v1 wire fixture"
    );
}

#[test]
fn v1_wire_fixtures_are_immutable_and_typed() {
    let scope = ProcessScopeV1::new(
        id::<TenantId>("tnt_market"),
        id::<ProcessId>("prc_trade"),
        id::<DefinitionId>("def_trade"),
        id::<DefinitionVersion>("dfv_one"),
        ContentDigest([1; 32]),
    );
    let input = ProcessInputDtoV1::new(
        id("tnt_market"),
        id("prc_trade"),
        id::<InputId>("inp_start"),
        ProcessInputKindV1::CanonicalEvent,
        ContentDigest([2; 32]),
    );

    assert_fixture(
        include_str!("fixtures/v1/process-definition.json"),
        ProcessDefinitionDtoV1::new(
            id("def_trade"),
            id("dfv_one"),
            ContentDigest([1; 32]),
            vec![id::<StepId>("stp_lock"), id("stp_settle")],
        )
        .expect("fixture definition is valid"),
        ProcessDefinitionDtoV1::validate,
    );
    assert_fixture(
        include_str!("fixtures/v1/process-input.json"),
        input.clone(),
        ProcessInputDtoV1::validate,
    );
    assert_fixture(
        include_str!("fixtures/v1/process-input-envelope.json"),
        ProcessInputEnvelopeV1::new(input),
        ProcessInputEnvelopeV1::validate,
    );
    assert_fixture(
        include_str!("fixtures/v1/process-outcome.json"),
        ProcessOutcomeDtoV1::new(
            scope.clone(),
            0,
            ProcessOutcomeFactV1::new(
                id::<OutcomeId>("out_started"),
                CausationIdV1::Input(id::<InputId>("inp_start")),
                OutcomeActorV1::System,
                LogicalTimeV1(7),
                ProcessOutcomeKindV1::Started,
                ContentDigest([3; 32]),
            ),
        ),
        ProcessOutcomeDtoV1::validate,
    );
    assert_fixture(
        include_str!("fixtures/v1/process-action.json"),
        ProcessActionDtoV1::new(
            scope.clone(),
            id::<ActionId>("act_lock"),
            id::<StepId>("stp_lock"),
            0,
            ProcessActionKindV1::CanonicalCommand,
            ContentDigest([4; 32]),
        ),
        ProcessActionDtoV1::validate,
    );
    assert_fixture(
        include_str!("fixtures/v1/canonical-command.json"),
        CanonicalCommandDtoV1::new(
            id("tnt_market"),
            id::<ActionId>("act_lock"),
            id::<OperationId>("op_lock"),
            vec![id::<ResourceId>("res_asset_a"), id("res_asset_b")],
            ContentDigest([5; 32]),
        )
        .expect("fixture command is valid"),
        CanonicalCommandDtoV1::validate,
    );
    assert_fixture(
        include_str!("fixtures/v1/canonical-event.json"),
        CanonicalEventDtoV1::new(
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
        CanonicalEventDtoV1::validate,
    );
    assert_fixture(
        include_str!("fixtures/v1/manual-review.json"),
        ManualReviewDtoV1::new(
            scope,
            id::<ReviewId>("rev_trade"),
            10,
            Some(LogicalTimeV1(42)),
            ContentDigest([7; 32]),
        ),
        ManualReviewDtoV1::validate,
    );
}
