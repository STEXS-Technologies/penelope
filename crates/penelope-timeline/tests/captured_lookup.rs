//! Optimized lookup must preserve complete descriptor and payload checks.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use penelope_core::task::Digest;
use penelope_timeline::*;
use proptest::prelude::*;
use std::{cell::Cell, rc::Rc};

fn record(step: u64, value: u64) -> InputRecord {
    InputRecord {
        scope: TimelineScope {
            tenant: TimelineTenantId::from_digest(Digest([1; 32])),
            timeline: TimelineId::from_digest(Digest([2; 32])),
            model: TimelineModelId::from_digest(Digest([3; 32])),
            schema: TimelineSchemaId::from_digest(Digest([4; 32])),
        },
        branch: TimelineBranchId::ZERO,
        step: TimelineStep::new(step),
        identity: TimelineInputId::from_digest(Digest::of(&step.to_le_bytes())),
        revision: InputRevision::ZERO,
        digest: value.digest(),
    }
}

#[test]
fn slot_hint_never_substitutes_another_scope_branch_or_version() {
    let original = record(10, 7);
    let mut alternatives = [original; 9];
    alternatives[0].scope.tenant = TimelineTenantId::from_digest(Digest([11; 32]));
    alternatives[1].scope.timeline = TimelineId::from_digest(Digest([12; 32]));
    alternatives[2].scope.model = TimelineModelId::from_digest(Digest([13; 32]));
    alternatives[3].scope.schema = TimelineSchemaId::from_digest(Digest([14; 32]));
    alternatives[4].branch = TimelineBranchId::new(1);
    alternatives[5].step = TimelineStep::new(11);
    alternatives[6].identity = TimelineInputId::from_digest(Digest([15; 32]));
    alternatives[7].revision = InputRevision::new(1);
    alternatives[8].digest = 8_u64.digest();
    let inputs = [CapturedInput::new(original, 7_u64).unwrap()];
    for substituted in alternatives {
        assert!(matches!(
            inputs.read(substituted),
            Err(TimelineError::HistoryUnavailable)
        ));
    }
    // The guessed slot has the original version, while a corrected version is
    // retained elsewhere at the same step. The fallback must find that exact one.
    let replacement = alternatives[7];
    let versions = [
        CapturedInput::new(original, 7_u64).unwrap(),
        CapturedInput::new(replacement, 7_u64).unwrap(),
    ];
    assert_eq!(versions.read(replacement).unwrap().record, replacement);
}

#[test]
fn empty_unordered_gapped_and_counter_boundary_buffers_remain_supported() {
    let empty: [CapturedInput<u64>; 0] = [];
    assert!(matches!(
        empty.read(record(0, 1)),
        Err(TimelineError::HistoryUnavailable)
    ));
    for steps in [
        [0, 1, 2],
        [u64::MAX - 2, u64::MAX - 1, u64::MAX],
        [u64::MAX, 0, 9],
        [4, 90, 5],
        [10, 12, 11],
    ] {
        let inputs = steps.map(|step| CapturedInput::new(record(step, step), step).unwrap());
        for step in steps {
            let bound = inputs.read(record(step, step)).unwrap();
            assert_eq!(*bound.value, step);
        }
    }
}

struct MutablePayload(Rc<Cell<u64>>);
impl HistoryPayload for MutablePayload {
    fn digest(&self) -> Digest {
        self.0.get().digest()
    }
    fn byte_len(&self) -> u64 {
        8
    }
}
#[test]
fn both_lookup_paths_recheck_payload_integrity() {
    for ordered in [true, false] {
        let changed = Rc::new(Cell::new(9_u64));
        let other = Rc::new(Cell::new(8_u64));
        let target = record(10, 9);
        let first = CapturedInput::new(record(9, 8), MutablePayload(other)).unwrap();
        let second = CapturedInput::new(target, MutablePayload(Rc::clone(&changed))).unwrap();
        let third =
            CapturedInput::new(record(11, 8), MutablePayload(Rc::new(Cell::new(8)))).unwrap();
        let inputs = if ordered {
            [first, second, third]
        } else {
            // Step 10 guesses slot 1, which contains step 11: force fallback.
            [first, third, second]
        };
        assert_eq!(inputs.read(target).unwrap().value.0.get(), 9);
        changed.set(99);
        assert!(matches!(
            inputs.read(target),
            Err(TimelineError::InputMismatch)
        ));
    }
}

proptest! {
    #[test]
    fn contiguous_windows_match_linear_search_at_every_offset(
        start in 0_u64..(u64::MAX - 80),
        values in prop::collection::vec(any::<u64>(), 0..80),
    ) {
        let inputs: Vec<_> = values.iter().enumerate().map(|(offset, value)| {
            let step = start.checked_add(u64::try_from(offset).unwrap()).unwrap();
            CapturedInput::new(record(step, *value), *value).unwrap()
        }).collect();
        for (index, input) in inputs.iter().enumerate() {
            let bound = inputs.as_slice().read(input.record()).unwrap();
            prop_assert_eq!(bound.record, input.record());
            prop_assert_eq!(*bound.value, values[index]);
        }
    }

    #[test]
    fn arbitrary_layouts_match_exact_record_search(
        layout in prop::collection::vec((any::<u64>(), any::<u64>(), 0_u64..4), 0..80),
        query in any::<u64>(),
    ) {
        let records: Vec<_> = layout.iter().map(|(step, value, branch)| {
            let mut descriptor = record(*step, *value);
            descriptor.branch = TimelineBranchId::new(*branch);
            descriptor
        }).collect();
        let inputs: Vec<_> = records.iter().zip(&layout).map(|(descriptor, (_, value, _))|
            CapturedInput::new(*descriptor, *value).unwrap()).collect();
        // Query every retained version and an arbitrary absent descriptor. Compare
        // with the original full linear search rather than a second slot formula.
        for requested in records.iter().copied().chain(core::iter::once(record(query, query))) {
            let expected = records.iter().position(|descriptor| *descriptor == requested);
            let actual = inputs.as_slice().read(requested);
            match expected {
                Some(index) => {
                    let bound = actual.unwrap();
                    prop_assert_eq!(bound.record, requested);
                    prop_assert_eq!(*bound.value, layout[index].1);
                }
                None => prop_assert!(matches!(actual, Err(TimelineError::HistoryUnavailable))),
            }
        }
    }
}
