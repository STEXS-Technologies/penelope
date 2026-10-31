//! Isolated executable: verify short identity parsing/cloning has no allocation.
#![allow(clippy::unwrap_used)]
use penelope_domain::*;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Allocator;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
// SAFETY: pointers/layouts pass unchanged to System; the counter cannot allocate.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller's valid layout is forwarded unchanged.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the pointer and its original layout are forwarded unchanged.
        unsafe { System.dealloc(ptr, layout) }
    }
}
fn count<T>(operation: impl FnOnce() -> T) -> usize {
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    drop(black_box(operation()));
    ALLOCATIONS.load(Ordering::Relaxed).saturating_sub(before)
}
fn main() {
    macro_rules! check {
        ($kind:ty, $text:literal) => {{
            let value: $kind = $text.parse().unwrap();
            assert_eq!(count(|| <$kind>::try_from(black_box($text)).unwrap()), 0);
            assert_eq!(count(|| black_box(&value).clone()), 0);
            let json = serde_json::to_string($text).unwrap();
            assert_eq!(
                count(|| serde_json::from_str::<$kind>(black_box(&json)).unwrap()),
                0
            );
            let long = format!("{}{}", $text, "x".repeat(64));
            let value: $kind = long.parse().unwrap();
            assert_eq!(count(|| long.parse::<$kind>().unwrap()), 1);
            assert_eq!(count(|| black_box(&value).clone()), 1);
            // Inline capacity follows pointer width rather than growing 32-bit values.
            assert_eq!(size_of::<$kind>(), size_of::<String>());
        }};
    }
    check!(TenantId, "tnt_a");
    check!(ProcessId, "prc_a");
    check!(DefinitionId, "def_a");
    check!(DefinitionVersion, "dfv_a");
    check!(StepId, "stp_a");
    check!(InputId, "inp_a");
    check!(OutcomeId, "out_a");
    check!(ActionId, "act_a");
    check!(ReviewId, "rev_a");
    check!(PrincipalId, "pri_a");
    check!(CanonicalEventId, "cev_a");
    check!(CanonicalCommitId, "cmt_a");
    check!(ResourceId, "res_a");
    check!(OperationId, "op_a");
    check!(ExternalReferenceId, "ext_a");
    check!(MigrationId, "mig_a");
    let scope = ProcessScope::new(
        "tnt_bench".parse().unwrap(),
        "prc_bench".parse().unwrap(),
        "def_bench".parse().unwrap(),
        "dfv_one".parse().unwrap(),
        ContentDigest([3; 32]),
    );
    assert_eq!(count(|| black_box(&scope).canonical_bytes()), 1);
    let maximum_scope = ProcessScope::new(
        format!("tnt_{}", "x".repeat(124)).parse().unwrap(),
        format!("prc_{}", "x".repeat(124)).parse().unwrap(),
        format!("def_{}", "x".repeat(124)).parse().unwrap(),
        format!("dfv_{}", "x".repeat(124)).parse().unwrap(),
        ContentDigest([3; 32]),
    );
    assert_eq!(count(|| black_box(&maximum_scope).canonical_bytes()), 1);
    let outcome = ProcessOutcome::new(
        scope,
        0,
        ProcessOutcomeFact::new(
            "out_bench_0".parse().unwrap(),
            CausationId::Action("act_bench".parse().unwrap()),
            OutcomeActor::System,
            LogicalTime(0),
            ProcessOutcomeKind::ActionPlanned,
            ContentDigest([4; 32]),
        ),
    );
    assert_eq!(
        count(|| black_box(&outcome).canonical_wire_bytes().unwrap()),
        1
    );
    assert_eq!(
        count(|| ContentDigest([255; 32]).canonical_wire_bytes().unwrap()),
        1
    );
    println!(
        "All 16 identifier types: short borrowed parse/JSON decode/clone zero allocations; long parse/clone one allocation; unchanged value size"
    );
}
