//! Isolated executable: definition admission remains allocation-free.
#![allow(clippy::unwrap_used)]
use penelope_domain::ContentDigest;
use penelope_executor::engine::{EngineError, LinearSagaDefinition, RetryPolicy, StepPlan};
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
    for long in [false, true] {
        let prefix = if long { "x".repeat(96) } else { "x".into() };
        for length in [1, 16, 17, 128] {
            let mut definition = LinearSagaDefinition {
                definition_id: "def_alloc".parse().unwrap(),
                definition_version: "dfv_one".parse().unwrap(),
                definition_digest: ContentDigest([1; 32]),
                steps: (0..length)
                    .rev()
                    .map(|key| {
                        StepPlan::canonical_command(
                            format!("stp_{prefix}_{key:03}").parse().unwrap(),
                            ContentDigest([2; 32]),
                            RetryPolicy::no_retry(),
                        )
                    })
                    .collect(),
            };
            assert_eq!(count(|| black_box(&definition).validate().unwrap()), 0);
            if length > 1 {
                let duplicate = definition.steps.first().unwrap().step_id.clone();
                definition.steps.last_mut().unwrap().step_id = duplicate;
                assert_eq!(
                    count(|| assert_eq!(
                        black_box(&definition).validate(),
                        Err(EngineError::DuplicateStepId)
                    )),
                    0
                );
            }
        }
    }
    println!(
        "Linear definition admission: zero allocations for valid and duplicate fixtures across the sorting threshold, short and long identifiers"
    );
}
