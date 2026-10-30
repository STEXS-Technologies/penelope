//! Frozen pairwise validator from 5201529; exact errors and input immutability.
#![allow(clippy::unwrap_used)]
use penelope_domain::{ContentDigest, MAX_ACTION_ATTEMPTS_PER_STEP, MAX_DEFINITION_STEPS};
use penelope_executor::engine::{
    CompensationPlan, EngineError, LinearSagaDefinition, RetryPolicy, StepPlan,
};
use proptest::prelude::*;
use std::num::NonZeroU32;
fn reference(definition: &LinearSagaDefinition) -> Result<(), EngineError> {
    if definition.steps.is_empty() {
        return Err(EngineError::EmptyDefinition);
    }
    if definition.steps.len() > MAX_DEFINITION_STEPS {
        return Err(EngineError::DefinitionStepLimitExceeded);
    }
    if definition.steps.iter().enumerate().any(|(index, step)| {
        definition
            .steps
            .iter()
            .skip(index.saturating_add(1))
            .any(|other_step| other_step.step_id == step.step_id)
    }) {
        return Err(EngineError::DuplicateStepId);
    }
    if definition.steps.iter().any(|step| {
        step.retry_policy.max_attempts.get() > MAX_ACTION_ATTEMPTS_PER_STEP
            || step.compensation.is_some_and(|compensation| {
                compensation.retry_policy.max_attempts.get() > MAX_ACTION_ATTEMPTS_PER_STEP
            })
    }) {
        return Err(EngineError::RetryAttemptLimitExceeded);
    }
    Ok(())
}
fn definition(fields: &[(u16, u32, Option<u32>)], long: bool) -> LinearSagaDefinition {
    let prefix = if long { "x".repeat(96) } else { "x".into() };
    LinearSagaDefinition {
        definition_id: "def_parity".parse().unwrap(),
        definition_version: "dfv_one".parse().unwrap(),
        definition_digest: ContentDigest([1; 32]),
        steps: fields
            .iter()
            .map(|(key, attempts, compensation)| {
                let mut retry = RetryPolicy::no_retry();
                retry.max_attempts = NonZeroU32::new(*attempts).unwrap();
                let mut step = StepPlan::canonical_command(
                    format!("stp_{prefix}_{key:03}").parse().unwrap(),
                    ContentDigest([2; 32]),
                    retry,
                );
                step.compensation = compensation.map(|compensation_attempts| {
                    let mut compensation_retry = RetryPolicy::no_retry();
                    compensation_retry.max_attempts =
                        NonZeroU32::new(compensation_attempts).unwrap();
                    CompensationPlan::canonical_command(ContentDigest([3; 32]), compensation_retry)
                });
                step
            })
            .collect(),
    }
}
proptest! {
    #[test]
    fn arbitrary_definitions_preserve_exact_errors_and_source_order(
        fields in prop::collection::vec((0u16..132, 1u32..=65, prop::option::of(1u32..=65)), 0..=129),
        long in any::<bool>(),
    ) {
        let definition = definition(&fields, long);
        let before = definition.clone();
        prop_assert_eq!(definition.validate(), reference(&definition));
        prop_assert_eq!(definition, before);
    }
    #[test]
    fn unique_definitions_and_injected_duplicates_preserve_admission(
        count in 1u16..=128, reverse in any::<bool>(), long in any::<bool>(), key in any::<u16>(),
    ) {
        let mut fields: Vec<_> = (0..count).map(|value| (value, 1, None)).collect();
        if reverse { fields.reverse(); }
        let mut definition = definition(&fields, long);
        prop_assert_eq!(reference(&definition), Ok(()));
        prop_assert_eq!(definition.validate(), Ok(()));
        if count > 1 {
            let index = usize::from(key.checked_rem(count.checked_sub(1).unwrap()).unwrap());
            let duplicate = definition.steps.get(index).unwrap().step_id.clone();
            definition.steps.last_mut().unwrap().step_id = duplicate;
            prop_assert_eq!(reference(&definition), Err(EngineError::DuplicateStepId));
            prop_assert_eq!(definition.validate(), reference(&definition));
        }
    }
}
#[test]
fn boundaries_and_mixed_invalid_fields_preserve_error_precedence() {
    for count in [0_u16, 1, 2, 8, 9, 16, 17, 32, 127, 128, 129] {
        for long in [false, true] {
            let fields: Vec<_> = (0..count).map(|key| (key, 1, None)).collect();
            let mut definition = definition(&fields, long);
            assert_eq!(definition.validate(), reference(&definition));
            if let Some(first) = definition.steps.first_mut() {
                first.retry_policy.max_attempts =
                    NonZeroU32::new(MAX_ACTION_ATTEMPTS_PER_STEP.checked_add(1).unwrap()).unwrap();
            }
            assert_eq!(definition.validate(), reference(&definition));
            if count > 1 {
                let duplicate = definition.steps.first().unwrap().step_id.clone();
                definition.steps.last_mut().unwrap().step_id = duplicate;
                let expected = if usize::from(count) > MAX_DEFINITION_STEPS {
                    EngineError::DefinitionStepLimitExceeded
                } else {
                    EngineError::DuplicateStepId
                };
                assert_eq!(definition.validate(), Err(expected));
                assert_eq!(definition.validate(), reference(&definition));
            }
        }
    }
}
