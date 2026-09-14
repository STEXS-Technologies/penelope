//! Fault-injection drill for the atomic process-store port contract.
//!
//! This is deliberately a test double, not a shipped adapter. It exercises the
//! transactional invariants an outer durable adapter must preserve.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::task::{Context, Poll, Waker};

use async_trait::async_trait;
use penelope_domain::{
    ActionId, CausationIdV1, ContentDigest, DefinitionId, DefinitionVersion, InputId,
    LogicalTimeV1, OutcomeActorV1, OutcomeId, ProcessActionDtoV1, ProcessActionKindV1, ProcessId,
    ProcessInputDtoV1, ProcessInputKindV1, ProcessOutcomeDtoV1, ProcessOutcomeFactV1,
    ProcessOutcomeKindV1, ProcessScopeV1, StepId, TenantId,
};
use penelope_ports::{
    AtomicProcessCommitReceiptV1, AtomicProcessCommitV1, OutcomeReplayPageV1,
    OutcomeReplayRequestV1, PortError, ProcessStore,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct StoreState {
    next_sequence: u64,
    inputs: Vec<InputId>,
    outcomes: Vec<ProcessOutcomeDtoV1>,
    actions: Vec<ProcessActionDtoV1>,
}

struct FaultInjectingStore {
    state: Mutex<StoreState>,
    fail_before_commit: AtomicBool,
}

impl FaultInjectingStore {
    const fn new() -> Self {
        Self {
            state: Mutex::new(StoreState {
                next_sequence: 0,
                inputs: Vec::new(),
                outcomes: Vec::new(),
                actions: Vec::new(),
            }),
            fail_before_commit: AtomicBool::new(false),
        }
    }

    fn fail_next_commit(&self) {
        self.fail_before_commit.store(true, Ordering::Release);
    }

    fn snapshot(&self) -> StoreState {
        self.state.lock().expect("test mutex poisoned").clone()
    }

    fn lock(&self) -> MutexGuard<'_, StoreState> {
        self.state.lock().expect("test mutex poisoned")
    }
}

#[async_trait]
impl ProcessStore for FaultInjectingStore {
    async fn commit(
        &self,
        commit: &AtomicProcessCommitV1,
    ) -> Result<AtomicProcessCommitReceiptV1, PortError> {
        commit.validate().map_err(|_error| PortError::Invariant)?;
        if self.fail_before_commit.swap(false, Ordering::AcqRel) {
            return Err(PortError::Unavailable);
        }
        let mut current = self.lock();
        if let Some(input) = &commit.input
            && current.inputs.iter().any(|seen| seen == &input.input_id)
        {
            return Ok(AtomicProcessCommitReceiptV1 {
                committed_through_sequence: current.next_sequence.saturating_sub(1),
                duplicate_input: true,
            });
        }
        if commit.expected_sequence != current.next_sequence {
            return Err(PortError::Conflict);
        }
        if commit.actions.iter().any(|action| {
            current.actions.iter().any(|stored| {
                stored.action_id == action.action_id || stored.effect_key() == action.effect_key()
            })
        }) {
            return Err(PortError::Invariant);
        }
        let mut staged = current.clone();
        if let Some(input) = &commit.input {
            staged.inputs.push(input.input_id.clone());
        }
        staged.next_sequence = staged
            .next_sequence
            .checked_add(
                u64::try_from(commit.outcomes.len()).map_err(|_error| PortError::Invariant)?,
            )
            .ok_or(PortError::Invariant)?;
        staged.outcomes.extend(commit.outcomes.clone());
        staged.actions.extend(commit.actions.clone());
        let receipt = AtomicProcessCommitReceiptV1 {
            committed_through_sequence: staged.next_sequence.saturating_sub(1),
            duplicate_input: false,
        };
        *current = staged;
        Ok(receipt)
    }

    async fn append_outcomes(
        &self,
        expected_sequence: u64,
        outcomes: &[ProcessOutcomeDtoV1],
    ) -> Result<(), PortError> {
        let commit =
            AtomicProcessCommitV1::new(expected_sequence, None, outcomes.to_vec(), Vec::new())
                .map_err(|_error| PortError::Invariant)?;
        self.commit(&commit).await.map(|_| ())
    }

    async fn read_outcomes(
        &self,
        request: &OutcomeReplayRequestV1,
    ) -> Result<OutcomeReplayPageV1, PortError> {
        request.validate().map_err(|_error| PortError::Invariant)?;
        let current = self.lock();
        let outcomes = current
            .outcomes
            .iter()
            .filter(|outcome| outcome.sequence >= request.from_sequence)
            .take(usize::from(request.limit.get()))
            .cloned()
            .collect::<Vec<_>>();
        if outcomes.iter().any(|outcome| {
            outcome.tenant_id != request.scope.tenant_id
                || outcome.process_id != request.scope.process_id
                || outcome.definition_id != request.scope.definition_id
                || outcome.definition_version != request.scope.definition_version
                || outcome.definition_digest != request.scope.definition_digest
        }) {
            return Err(PortError::Invariant);
        }
        let next_sequence = outcomes.last().and_then(|outcome| {
            outcome
                .sequence
                .checked_add(1)
                .filter(|next| *next < current.next_sequence)
        });
        OutcomeReplayPageV1::new(
            request.scope.clone(),
            request.from_sequence,
            outcomes,
            next_sequence,
        )
        .map_err(|_error| PortError::Invariant)
    }
}

fn block_on<T>(future: impl Future<Output = T>) -> T {
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    let mut future = std::pin::pin!(future);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("test double must complete without scheduling"),
    }
}

fn id<T: TryFrom<&'static str>>(value: &'static str) -> T {
    T::try_from(value).ok().expect("valid test identifier")
}

fn scope() -> ProcessScopeV1 {
    ProcessScopeV1::new(
        id::<TenantId>("tnt_fault"),
        id::<ProcessId>("prc_fault"),
        id::<DefinitionId>("def_fault"),
        id::<DefinitionVersion>("dfv_one"),
        ContentDigest([9; 32]),
    )
}

fn commit() -> AtomicProcessCommitV1 {
    let scope = scope();
    let input = ProcessInputDtoV1::new(
        scope.tenant_id.clone(),
        scope.process_id.clone(),
        id::<InputId>("inp_fault"),
        ProcessInputKindV1::CanonicalEvent,
        ContentDigest([1; 32]),
    );
    let outcome = ProcessOutcomeDtoV1::new(
        scope.clone(),
        0,
        ProcessOutcomeFactV1::new(
            id::<OutcomeId>("out_fault"),
            CausationIdV1::Input(input.input_id.clone()),
            OutcomeActorV1::System,
            LogicalTimeV1(1),
            ProcessOutcomeKindV1::Started,
            ContentDigest([2; 32]),
        ),
    );
    let action = ProcessActionDtoV1::new(
        scope,
        id::<ActionId>("act_fault"),
        id::<StepId>("stp_fault"),
        0,
        ProcessActionKindV1::CanonicalCommand,
        ContentDigest([3; 32]),
    );
    AtomicProcessCommitV1::new(0, Some(input), vec![outcome], vec![action])
        .expect("valid atomic commit")
}

#[test]
fn injected_failure_is_atomic_and_recovery_replays_one_committed_input() {
    let store = FaultInjectingStore::new();
    let commit = commit();
    let before = store.snapshot();
    store.fail_next_commit();
    assert_eq!(block_on(store.commit(&commit)), Err(PortError::Unavailable));
    assert_eq!(store.snapshot(), before);

    let receipt = block_on(store.commit(&commit)).expect("commit recovers");
    assert_eq!(receipt.committed_through_sequence, 0);
    assert!(!receipt.duplicate_input);
    let duplicate = block_on(store.commit(&commit)).expect("duplicate is idempotent");
    assert!(duplicate.duplicate_input);
    assert_eq!(store.snapshot().outcomes.len(), 1);
    assert_eq!(store.snapshot().actions.len(), 1);

    let request = OutcomeReplayRequestV1::new(scope(), 0, std::num::NonZeroU16::MIN)
        .expect("bounded replay request");
    let page = block_on(store.read_outcomes(&request)).expect("replay succeeds after recovery");
    assert_eq!(page.outcomes.len(), 1);
    let first_outcome = page.outcomes.first().expect("one replayed outcome");
    assert_eq!(first_outcome.sequence, 0);

    let conflict = AtomicProcessCommitV1::new(0, None, vec![first_outcome.clone()], Vec::new())
        .expect("locally valid stale commit");
    assert_eq!(block_on(store.commit(&conflict)), Err(PortError::Conflict));
}
