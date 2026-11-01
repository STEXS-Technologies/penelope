//! Prepare a new live step on isolated scratch, using the same computation contract.
use crate::*;
use thiserror::Error;

/// Consumer-owned scratch and exact post-head restore anchor for one new input.
pub struct LiveStepRequest<State> {
    pub expected: TimelineHead,
    pub anchor: StateAnchor,
    pub input: InputRecord,
    pub scratch: State,
    pub limits: EffectLimits,
}

/// Opaque validated live increment. Computation alone never publishes it.
pub struct ReadyStep<State, Effect, const EFFECTS: usize> {
    pub(crate) expected: TimelineHead,
    pub(crate) input: InputRecord,
    pub(crate) state: State,
    pub(crate) effects: StagedEffects<Effect, EFFECTS>,
}

/// Independent concrete errors for live-step preparation.
#[derive(Debug, Error)]
pub enum StepError<ModelError, InputError, SnapshotError> {
    #[error(transparent)]
    Timeline(#[from] TimelineError),
    #[error("consumer computation failed")]
    Computation(ModelError),
    #[error("consumer input access failed")]
    Input(InputError),
    #[error("consumer snapshot restoration failed")]
    Snapshot(SnapshotError),
}

/// Restores the current complete state, computes and validates one new logical input.
/// On any error/unwind only scratch and tentative intents can change.
/// # Errors
/// Refuses stale/malformed descriptors, unavailable state/input, computation,
/// effect-budget and invariant failures. Publication still performs an exact-head CAS.
#[allow(clippy::type_complexity)] // Preserve each adapter's concrete failure type.
pub fn prepare_live_step<M, I, S, V, const EFFECTS: usize>(
    mut request: LiveStepRequest<M::State>,
    model: &M,
    inputs: &I,
    snapshots: &S,
    validator: &V,
) -> Result<ReadyStep<M::State, M::Effect, EFFECTS>, StepError<M::Error, I::Error, S::Error>>
where
    M: RewindableComputation,
    I: InputStore<M::Input>,
    S: LiveSnapshotStore<M::State>,
    V: CandidateValidator<M::State>,
{
    let mut check = TimelineProjection::<0>::new(request.expected);
    check.append(request.expected, request.input)?;
    if request.anchor.scope != request.expected.scope()
        || request.anchor.branch != request.expected.branch()
    {
        return Err(TimelineError::ScopeMismatch.into());
    }
    if request.anchor.step != request.expected.step() {
        return Err(TimelineError::HistoryUnavailable.into());
    }
    let restored = snapshots
        .restore_current(request.expected, request.anchor, &mut request.scratch)
        .map_err(StepError::Snapshot)?;
    if restored != request.anchor {
        return Err(TimelineError::InputMismatch.into());
    }
    let input = inputs.read(request.input).map_err(StepError::Input)?;
    if input.record != request.input {
        return Err(TimelineError::InputMismatch.into());
    }
    let mut effects = StagedEffects::new(request.limits);
    let context = StepContext {
        revision: check.head().revision(),
        scope: request.expected.scope(),
        branch: request.expected.branch(),
        step: request.input.step,
    };
    let mut sink = effects.sink(context);
    model
        .step(&mut request.scratch, input.value, context, &mut sink)
        .map_err(StepError::Computation)?;
    if sink.poisoned {
        return Err(TimelineError::Capacity.into());
    }
    validator.validate(&request.scratch, check.head())?;
    Ok(ReadyStep {
        expected: request.expected,
        input: request.input,
        state: request.scratch,
        effects,
    })
}
