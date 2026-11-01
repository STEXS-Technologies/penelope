//! Exact lookup remains independent of slot layout, imports and floor movement.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
#[path = "support/history_lookup.rs"]
mod lookup;
use proptest::prelude::*;
proptest! {
    #[test]
    fn retained_versions_match_linear_oracle(data in prop::collection::vec(any::<u8>(), 0..128)) {
        lookup::run(&data);
    }
}
#[test]
fn counter_edge_and_every_recycled_slot_remain_readable() {
    for expired in 0..=16 {
        let mut trace = vec![255; 64];
        trace[1] = expired;
        lookup::run(&trace);
    }
}

#[test]
fn canonical_and_exact_lookups_recheck_content_and_declared_charge() {
    use penelope_core::task::Digest;
    use penelope_timeline::*;
    use std::{cell::Cell, rc::Rc};
    struct Mutable {
        value: Rc<Cell<u64>>,
        bytes: Rc<Cell<u64>>,
    }
    impl HistoryPayload for Mutable {
        fn digest(&self) -> Digest {
            self.value.get().digest()
        }
        fn byte_len(&self) -> u64 {
            self.bytes.get()
        }
    }
    let scope = TimelineScope {
        tenant: TimelineTenantId::from_digest(Digest([1; 32])),
        timeline: TimelineId::from_digest(Digest([2; 32])),
        model: TimelineModelId::from_digest(Digest([3; 32])),
        schema: TimelineSchemaId::from_digest(Digest([4; 32])),
    };
    for interleaved in [false, true] {
        let anchor = StateAnchor {
            scope,
            branch: TimelineBranchId::ZERO,
            step: TimelineStep::ZERO,
            snapshot: TimelineSnapshotId::from_digest(Digest([5; 32])),
            digest: 0_u64.digest(),
        };
        let mut history = FixedHistory::<u64, Mutable, 8, 2, 1>::new(
            anchor,
            0,
            HistoryLimits {
                max_input_bytes: 16,
                max_snapshot_bytes: 8,
                max_total_bytes: 256,
            },
        )
        .unwrap();
        let mut cells = Vec::new();
        for step in 1..=3 {
            let value = Rc::new(Cell::new(step));
            let bytes = Rc::new(Cell::new(8));
            let record = InputRecord {
                scope,
                branch: TimelineBranchId::ZERO,
                step: TimelineStep::new(step),
                identity: TimelineInputId::from_digest(step.digest()),
                revision: InputRevision::ZERO,
                digest: step.digest(),
            };
            history
                .append_input(
                    record,
                    Mutable {
                        value: Rc::clone(&value),
                        bytes: Rc::clone(&bytes),
                    },
                )
                .unwrap();
            cells.push((record, value, bytes));
            if interleaved && step == 1 {
                let changed = InputRecord {
                    revision: InputRevision::new(1),
                    digest: 99_u64.digest(),
                    ..record
                };
                history
                    .import_input(
                        changed,
                        Mutable {
                            value: Rc::new(Cell::new(99)),
                            bytes: Rc::new(Cell::new(8)),
                        },
                    )
                    .unwrap();
            }
        }
        let charged = history.payload_bytes();
        for (record, value, bytes) in &cells {
            assert_eq!(
                history.read(*record).unwrap().value.value.get(),
                record.step.get()
            );
            bytes.set(0);
            assert!(matches!(
                history.read(*record),
                Err(TimelineError::InputMismatch)
            ));
            assert!(matches!(
                history.input_at(record.branch, record.step),
                Err(TimelineError::InputMismatch)
            ));
            bytes.set(8);
            value.set(999);
            assert!(matches!(
                history.read(*record),
                Err(TimelineError::InputMismatch)
            ));
            assert!(matches!(
                history.input_at(record.branch, record.step),
                Err(TimelineError::InputMismatch)
            ));
            value.set(record.step.get());
        }
        assert_eq!(history.payload_bytes(), charged);
    }
}
