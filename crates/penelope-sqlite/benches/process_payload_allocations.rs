//! Rust allocation traffic for public payload operations; no timing claims.
#![allow(unsafe_code, clippy::arithmetic_side_effects)]
#[path = "support/payload_fixture.rs"]
mod fixture;
use fixture::{Error, run};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
struct CountingAllocator;
static ENABLED: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static REALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static REQUESTED_BYTES: AtomicUsize = AtomicUsize::new(0);
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;
// SAFETY: each operation forwards the original pointer/layout to System. The
// instrumentation uses only atomics and cannot allocate, unwind or recurse.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if ENABLED.load(Ordering::SeqCst) {
            ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
            REQUESTED_BYTES.fetch_add(layout.size(), Ordering::SeqCst);
        }
        // SAFETY: caller supplies the GlobalAlloc layout contract unchanged.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if ENABLED.load(Ordering::SeqCst) {
            ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
            REQUESTED_BYTES.fetch_add(layout.size(), Ordering::SeqCst);
        }
        // SAFETY: caller supplies the GlobalAlloc layout contract unchanged.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if ENABLED.load(Ordering::SeqCst) {
            REALLOCATIONS.fetch_add(1, Ordering::SeqCst);
            REQUESTED_BYTES.fetch_add(size, Ordering::SeqCst);
        }
        // SAFETY: pointer, old layout and new size retain the caller's contract.
        unsafe { System.realloc(pointer, layout, size) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: pointer/layout are forwarded to their original allocator.
        unsafe { System.dealloc(pointer, layout) }
    }
}
fn main() -> Result<(), Error> {
    run(|fixture, case| {
        for _ in 0..16 {
            fixture.validate(case, fixture.call(case)?)?;
        }
        for sample in 0..16 {
            ALLOCATIONS.store(0, Ordering::SeqCst);
            REALLOCATIONS.store(0, Ordering::SeqCst);
            REQUESTED_BYTES.store(0, Ordering::SeqCst);
            ENABLED.store(true, Ordering::SeqCst);
            let result = fixture.call(case);
            ENABLED.store(false, Ordering::SeqCst);
            let allocations = ALLOCATIONS.load(Ordering::SeqCst);
            let reallocations = REALLOCATIONS.load(Ordering::SeqCst);
            let requested_bytes = REQUESTED_BYTES.load(Ordering::SeqCst);
            fixture.validate(case, result?)?;
            println!(
                "{}",
                serde_json::json!({"size":fixture.size(),"case":case.name(),"sample":sample,"allocations":allocations,"reallocations":reallocations,"requested_bytes":requested_bytes,"removed_bytes":fixture.removed_bytes(case)})
            );
        }
        Ok(())
    })
}
