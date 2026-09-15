//! Fault-injection drill for the atomic process-store port contract.
//!
//! This is deliberately a test double, not a shipped adapter. It exercises the
//! transactional invariants an outer durable adapter must preserve.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::future::Future;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::task::{Context, Poll, Waker};

use async_trait::async_trait;
use penelope_domain::{
    ActionId, CanonicalEventId, CausationId, ContentDigest, DefinitionId, DefinitionVersion,
    InputId, LogicalTime, OutcomeActor, OutcomeId, ProcessAction, ProcessActionKind, ProcessId,
    ProcessInput, ProcessInputKind, ProcessOutcome, ProcessOutcomeFact, ProcessOutcomeKind,
    ProcessScope, StepId, TenantId,
};
use penelope_ports::{
    AtomicProcessCommit, AtomicProcessCommitReceipt, OutcomeReplayPage, OutcomeReplayRequest,
    PortError, ProcessStore,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct StoreState {
    next_sequence: u64,
    inputs: Vec<InputId>,
    canonical_source_events: Vec<CanonicalEventId>,
    outcomes: Vec<ProcessOutcome>,
    actions: Vec<ProcessAction>,
}

/// Logical points before the test double makes one staged atomic commit visible.
///
/// These are deliberately test-only failpoints. A production adapter must
/// provide an equivalent transactional guarantee through its own durable
/// implementation rather than importing this fixture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum CommitFailpoint {
    BeforeInputStage = 1,
    AfterInputStage = 2,
    AfterOutcomeAppend = 3,
    AfterActionEnqueue = 4,
}

struct FaultInjectingStore {
    state: Mutex<StoreState>,
    failpoint: AtomicU8,
}

impl FaultInjectingStore {
    const fn new() -> Self {
        Self {
            state: Mutex::new(StoreState {
                next_sequence: 0,
                inputs: Vec::new(),
                canonical_source_events: Vec::new(),
                outcomes: Vec::new(),
                actions: Vec::new(),
            }),
            failpoint: AtomicU8::new(0),
        }
    }

    fn fail_next_commit_at(&self, failpoint: CommitFailpoint) {
        self.failpoint.store(failpoint as u8, Ordering::Release);
    }

    fn take_failpoint(&self, failpoint: CommitFailpoint) -> bool {
        self.failpoint
            .compare_exchange(failpoint as u8, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
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
        commit: &AtomicProcessCommit,
    ) -> Result<AtomicProcessCommitReceipt, PortError> {
        commit.validate().map_err(|_error| PortError::Invariant)?;
        let mut current = self.lock();
        if let Some(input) = &commit.input {
            if current.inputs.iter().any(|seen| seen == &input.input_id) {
                return Ok(AtomicProcessCommitReceipt {
                    committed_through_sequence: current.next_sequence.saturating_sub(1),
                    duplicate_input: true,
                });
            }
        }
        if commit
            .canonical_source_event_id
            .as_ref()
            .is_some_and(|source_event_id| {
                current
                    .canonical_source_events
                    .iter()
                    .any(|seen| seen == source_event_id)
            })
        {
            return Ok(AtomicProcessCommitReceipt {
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
            if self.take_failpoint(CommitFailpoint::BeforeInputStage) {
                return Err(PortError::Unavailable);
            }
            staged.inputs.push(input.input_id.clone());
            if let Some(source_event_id) = &commit.canonical_source_event_id {
                staged.canonical_source_events.push(source_event_id.clone());
            }
            if self.take_failpoint(CommitFailpoint::AfterInputStage) {
                return Err(PortError::Unavailable);
            }
        }
        staged.next_sequence = staged
            .next_sequence
            .checked_add(
                u64::try_from(commit.outcomes.len()).map_err(|_error| PortError::Invariant)?,
            )
            .ok_or(PortError::Invariant)?;
        staged.outcomes.extend(commit.outcomes.clone());
        if self.take_failpoint(CommitFailpoint::AfterOutcomeAppend) {
            return Err(PortError::Unavailable);
        }
        staged.actions.extend(commit.actions.clone());
        if self.take_failpoint(CommitFailpoint::AfterActionEnqueue) {
            return Err(PortError::Unavailable);
        }
        let receipt = AtomicProcessCommitReceipt {
            committed_through_sequence: staged.next_sequence.saturating_sub(1),
            duplicate_input: false,
        };
        *current = staged;
        Ok(receipt)
    }

    async fn append_outcomes(
        &self,
        scope: &ProcessScope,
        expected_sequence: u64,
        outcomes: &[ProcessOutcome],
    ) -> Result<(), PortError> {
        if outcomes.iter().any(|outcome| {
            outcome.tenant_id != scope.tenant_id
                || outcome.process_id != scope.process_id
                || outcome.definition_id != scope.definition_id
                || outcome.definition_version != scope.definition_version
                || outcome.definition_digest != scope.definition_digest
        }) {
            return Err(PortError::Invariant);
        }
        let commit =
            AtomicProcessCommit::new(expected_sequence, None, outcomes.to_vec(), Vec::new())
                .map_err(|_error| PortError::Invariant)?;
        self.commit(&commit).await.map(|_| ())
    }

    async fn read_outcomes(
        &self,
        request: &OutcomeReplayRequest,
    ) -> Result<OutcomeReplayPage, PortError> {
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
        OutcomeReplayPage::new(
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

fn scope() -> ProcessScope {
    ProcessScope::new(
        id::<TenantId>("tnt_fault"),
        id::<ProcessId>("prc_fault"),
        id::<DefinitionId>("def_fault"),
        id::<DefinitionVersion>("dfv_one"),
        ContentDigest([9; 32]),
    )
}

fn commit() -> AtomicProcessCommit {
    let scope = scope();
    let input = ProcessInput::new(
        scope.tenant_id.clone(),
        scope.process_id.clone(),
        id::<InputId>("inp_fault"),
        ProcessInputKind::CanonicalEvent,
        ContentDigest([1; 32]),
    );
    let outcome = ProcessOutcome::new(
        scope.clone(),
        0,
        ProcessOutcomeFact::new(
            id::<OutcomeId>("out_fault"),
            CausationId::Input(input.input_id.clone()),
            OutcomeActor::System,
            LogicalTime(1),
            ProcessOutcomeKind::Started,
            ContentDigest([2; 32]),
        ),
    );
    let action = ProcessAction::new(
        scope,
        id::<ActionId>("act_fault"),
        id::<StepId>("stp_fault"),
        0,
        ProcessActionKind::CanonicalCommand,
        ContentDigest([3; 32]),
    );
    AtomicProcessCommit::new(0, Some(input), vec![outcome], vec![action])
        .expect("valid atomic commit")
        .with_canonical_source_event(id("cev_fault"))
}

#[test]
fn injected_failure_at_every_atomic_boundary_has_no_visible_partial_state() {
    for failpoint in [
        CommitFailpoint::BeforeInputStage,
        CommitFailpoint::AfterInputStage,
        CommitFailpoint::AfterOutcomeAppend,
        CommitFailpoint::AfterActionEnqueue,
    ] {
        let store = FaultInjectingStore::new();
        let commit = commit();
        let before = store.snapshot();
        store.fail_next_commit_at(failpoint);
        assert_eq!(block_on(store.commit(&commit)), Err(PortError::Unavailable));
        assert_eq!(store.snapshot(), before);

        let receipt = block_on(store.commit(&commit)).expect("commit recovers");
        assert_eq!(receipt.committed_through_sequence, 0);
        assert!(!receipt.duplicate_input);
        let duplicate = block_on(store.commit(&commit)).expect("duplicate is idempotent");
        assert!(duplicate.duplicate_input);
        let mut same_source_new_inbox_id = commit.clone();
        same_source_new_inbox_id
            .input
            .as_mut()
            .expect("commit has input")
            .input_id = id("inp_redelivery");
        same_source_new_inbox_id
            .outcomes
            .first_mut()
            .expect("commit has an outcome")
            .causation_id = penelope_domain::CausationId::Input(id("inp_redelivery"));
        let duplicate_source = block_on(store.commit(&same_source_new_inbox_id))
            .expect("canonical source redelivery is idempotent");
        assert!(duplicate_source.duplicate_input);
        assert_eq!(store.snapshot().outcomes.len(), 1);
        assert_eq!(store.snapshot().actions.len(), 1);

        let request = OutcomeReplayRequest::new(scope(), 0, std::num::NonZeroU16::MIN)
            .expect("bounded replay request");
        let page = block_on(store.read_outcomes(&request)).expect("replay succeeds after recovery");
        assert_eq!(page.outcomes.len(), 1);
        let first_outcome = page.outcomes.first().expect("one replayed outcome");
        assert_eq!(first_outcome.sequence, 0);

        let conflict = AtomicProcessCommit::new(0, None, vec![first_outcome.clone()], Vec::new())
            .expect("locally valid stale commit");
        assert_eq!(block_on(store.commit(&conflict)), Err(PortError::Conflict));
    }
}
