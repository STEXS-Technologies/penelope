//! Materialization validates actual clones and leaves its source retryable.
#![allow(clippy::unwrap_used, clippy::panic, clippy::arithmetic_side_effects)]
use penelope_core::task::Digest;
use penelope_timeline::*;
use std::{cell::Cell, rc::Rc};

struct Payload {
    value: Rc<Cell<u64>>,
    bytes: u64,
    clone_value: Option<u64>,
    clone_bytes: u64,
    clones: Rc<Cell<usize>>,
    panic_at: Rc<Cell<Option<usize>>>,
}
impl Clone for Payload {
    fn clone(&self) -> Self {
        let ordinal = self.clones.get() + 1;
        self.clones.set(ordinal);
        assert_ne!(self.panic_at.get(), Some(ordinal), "injected clone panic");
        Self {
            value: Rc::new(Cell::new(
                self.clone_value.unwrap_or_else(|| self.value.get()),
            )),
            bytes: self.clone_bytes,
            clone_value: None,
            clone_bytes: self.clone_bytes,
            clones: Rc::clone(&self.clones),
            panic_at: Rc::clone(&self.panic_at),
        }
    }
}
impl HistoryPayload for Payload {
    fn digest(&self) -> Digest {
        HistoryPayload::digest(&self.value.get())
    }
    fn byte_len(&self) -> u64 {
        self.bytes
    }
}
type History = FixedHistory<Payload, Payload, 2, 3, 1>;
const fn scope() -> TimelineScope {
    TimelineScope {
        tenant: TimelineTenantId::from_digest(Digest([1; 32])),
        timeline: TimelineId::from_digest(Digest([2; 32])),
        model: TimelineModelId::from_digest(Digest([3; 32])),
        schema: TimelineSchemaId::from_digest(Digest([4; 32])),
    }
}
fn anchor(step: u64) -> StateAnchor {
    StateAnchor {
        scope: scope(),
        branch: TimelineBranchId::ZERO,
        step: TimelineStep::new(step),
        snapshot: TimelineSnapshotId::from_digest(Digest::of(&step.to_le_bytes())),
        digest: HistoryPayload::digest(&step),
    }
}
fn input(step: u64) -> InputRecord {
    InputRecord {
        scope: scope(),
        branch: TimelineBranchId::ZERO,
        step: TimelineStep::new(step),
        identity: TimelineInputId::from_digest(Digest::of(&step.to_le_bytes())),
        revision: InputRevision::ZERO,
        digest: HistoryPayload::digest(&step),
    }
}
struct Fixture {
    history: History,
    clones: Rc<Cell<usize>>,
    panic_at: Rc<Cell<Option<usize>>>,
    last_value: Rc<Cell<u64>>,
}
fn fixture(changed: usize, bad_digest: bool, clone_bytes: u64, limits: HistoryLimits) -> Fixture {
    let clones = Rc::new(Cell::new(0));
    let panic_at = Rc::new(Cell::new(None));
    let last_value = Rc::new(Cell::new(2));
    let payload = |ordinal: usize, step: u64| Payload {
        value: if ordinal == 4 {
            Rc::clone(&last_value)
        } else {
            Rc::new(Cell::new(step))
        },
        bytes: 8,
        clone_value: (ordinal == changed && bad_digest).then_some(99),
        clone_bytes: if ordinal == changed { clone_bytes } else { 8 },
        clones: Rc::clone(&clones),
        panic_at: Rc::clone(&panic_at),
    };
    let mut history = History::new(anchor(0), payload(0, 0), limits).unwrap();
    for step in 1..=2 {
        history
            .append_input(input(step), payload((step * 2 - 1) as usize, step))
            .unwrap();
        history
            .insert_snapshot(anchor(step), payload((step * 2) as usize, step))
            .unwrap();
    }
    Fixture {
        history,
        clones,
        panic_at,
        last_value,
    }
}
const LIMITS: HistoryLimits = HistoryLimits {
    max_input_bytes: 32,
    max_snapshot_bytes: 32,
    max_total_bytes: 128,
};
fn assert_source(history: &History) {
    assert_eq!(history.payload_bytes(), 40);
    for step in 1..=2 {
        assert_eq!(
            history.input_at(TimelineBranchId::ZERO, TimelineStep::new(step)),
            Ok(input(step))
        );
        assert_eq!(
            history.snapshot_at(TimelineBranchId::ZERO, TimelineStep::new(step)),
            Ok(anchor(step))
        );
    }
}
#[test]
fn every_cloned_payload_is_validated_and_charged() {
    for changed in 0..5 {
        let bad = fixture(changed, true, 8, LIMITS);
        assert!(matches!(
            bad.history.materialize_branch(TimelineBranchId::ZERO),
            Err(TimelineError::InputMismatch)
        ));
        assert_source(&bad.history);
        let enlarged = fixture(changed, false, 16, LIMITS);
        let detached = enlarged
            .history
            .materialize_branch(TimelineBranchId::ZERO)
            .unwrap();
        assert_eq!(detached.payload_bytes(), 48);
        assert_source(&enlarged.history);
        let smaller = fixture(changed, false, 0, LIMITS);
        let compact = smaller
            .history
            .materialize_branch(TimelineBranchId::ZERO)
            .unwrap();
        assert_eq!(compact.payload_bytes(), 32);
        assert_source(&smaller.history);
        let oversized = fixture(changed, false, 33, LIMITS);
        assert!(matches!(
            oversized.history.materialize_branch(TimelineBranchId::ZERO),
            Err(TimelineError::Capacity)
        ));
        assert_source(&oversized.history);
        let aggregate = fixture(
            changed,
            false,
            16,
            HistoryLimits {
                max_total_bytes: 47,
                ..LIMITS
            },
        );
        assert!(matches!(
            aggregate.history.materialize_branch(TimelineBranchId::ZERO),
            Err(TimelineError::Capacity)
        ));
        assert_source(&aggregate.history);
        let overflow = fixture(
            changed,
            false,
            u64::MAX,
            HistoryLimits {
                max_input_bytes: u64::MAX,
                max_snapshot_bytes: u64::MAX,
                max_total_bytes: u64::MAX,
            },
        );
        assert!(matches!(
            overflow.history.materialize_branch(TimelineBranchId::ZERO),
            Err(TimelineError::Capacity)
        ));
        assert_source(&overflow.history);
    }
}
#[test]
fn corrupt_source_is_rejected_before_cloning() {
    let corrupt = fixture(99, false, 8, LIMITS);
    corrupt.last_value.set(99); // Intentional contract violation tests preflight integrity.
    assert!(matches!(
        corrupt.history.materialize_branch(TimelineBranchId::ZERO),
        Err(TimelineError::InputMismatch)
    ));
    assert_eq!(corrupt.clones.get(), 0);
    corrupt.last_value.set(2);
    assert_source(&corrupt.history);
}
#[test]
fn clone_panic_at_every_position_preserves_source_and_allows_retry() {
    for ordinal in 1..=5 {
        let injected = fixture(99, false, 8, LIMITS);
        injected.panic_at.set(Some(ordinal));
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _result = injected.history.materialize_branch(TimelineBranchId::ZERO);
            }))
            .is_err()
        );
        assert_source(&injected.history);
        injected.panic_at.set(None);
        let detached = injected
            .history
            .materialize_branch(TimelineBranchId::ZERO)
            .unwrap();
        assert_source(&detached);
        assert_source(&injected.history);
    }
}
