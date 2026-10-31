//! Reference validator pinned to 245da19; preserve original error precedence.
use penelope_domain::*;
use penelope_ports::*;
pub fn validate(commit: &AtomicProcessCommit) -> Result<(), CommitValidationError> {
    let Some(first_outcome) = commit.outcomes.first() else {
        return Err(CommitValidationError::EmptyOutcomes);
    };
    if commit.outcomes.len() > MAX_OUTCOMES_PER_ATOMIC_COMMIT {
        return Err(CommitValidationError::OutcomeLimitExceeded);
    }
    let mut expected_sequence = commit.expected_sequence;
    let expected_scope = first_outcome.scope();
    for (index, outcome) in commit.outcomes.iter().enumerate() {
        if outcome.validate().is_err() {
            return Err(CommitValidationError::InvalidOutcome);
        }
        if outcome.validate_for_scope(&expected_scope).is_err() {
            return Err(CommitValidationError::OutcomeScopeMismatch);
        }
        if outcome.sequence != expected_sequence {
            return Err(CommitValidationError::NonContiguousSequence);
        }
        if commit
            .outcomes
            .iter()
            .skip(index.saturating_add(1))
            .any(|other_outcome| other_outcome.outcome_id == outcome.outcome_id)
        {
            return Err(CommitValidationError::DuplicateOutcomeId);
        }
        expected_sequence = expected_sequence
            .checked_add(1)
            .ok_or(CommitValidationError::SequenceOverflow)?;
    }
    if commit.actions.len() > MAX_ACTIONS_PER_ATOMIC_COMMIT {
        return Err(CommitValidationError::ActionLimitExceeded);
    }
    for (index, action) in commit.actions.iter().enumerate() {
        if action.validate().is_err() {
            return Err(CommitValidationError::InvalidAction);
        }
        if action.tenant_id != first_outcome.tenant_id
            || action.process_id != first_outcome.process_id
            || action.definition_id != first_outcome.definition_id
            || action.definition_version != first_outcome.definition_version
            || action.definition_digest != first_outcome.definition_digest
        {
            return Err(CommitValidationError::ActionScopeMismatch);
        }
        if commit
            .actions
            .iter()
            .skip(index.saturating_add(1))
            .any(|other_action| other_action.action_id == action.action_id)
        {
            return Err(CommitValidationError::DuplicateActionId);
        }
        let effect_key = action.effect_key();
        if commit
            .actions
            .iter()
            .skip(index.saturating_add(1))
            .any(|other_action| other_action.effect_key() == effect_key)
        {
            return Err(CommitValidationError::DuplicateActionEffectKey);
        }
    }
    if let Some(input) = &commit.input {
        if input.validate().is_err() {
            return Err(CommitValidationError::InvalidInput);
        }
        if input.tenant_id != first_outcome.tenant_id
            || input.process_id != first_outcome.process_id
        {
            return Err(CommitValidationError::InputScopeMismatch);
        }
    }
    if commit.canonical_source_event_id.is_some() {
        let Some(input) = &commit.input else {
            return Err(CommitValidationError::CanonicalSourceWithoutInput);
        };
        if input.kind != ProcessInputKind::CanonicalEvent {
            return Err(CommitValidationError::CanonicalSourceWithWrongInputKind);
        }
    }
    match commit.input.as_ref() {
            Some(input) if !commit.outcomes.iter().any(|outcome| {
                matches!(&outcome.causation_id, penelope_domain::CausationId::Input(input_id) if input_id == &input.input_id)
            }) => {
                return Err(CommitValidationError::InputNotCausallyRecorded);
            }
            _ => {}
        }
    Ok(())
}
