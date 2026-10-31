//! Count only this thread's Rust allocation traffic, excluding database workers.
#![allow(unsafe_code)]
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};
struct Allocator;
thread_local! {
    static COUNTERS: Cell<(bool, usize, usize, usize)> = const { Cell::new((false, 0, 0, 0)) };
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
fn record(bytes: usize, reallocating: bool) {
    // Const TLS initialization allocates nothing. Teardown may make it unavailable.
    let _recorded = COUNTERS.try_with(|counters| {
        let (enabled, allocations, reallocations, requested) = counters.get();
        if enabled {
            counters.set((
                true,
                allocations.wrapping_add(usize::from(!reallocating)),
                reallocations.wrapping_add(usize::from(reallocating)),
                requested.wrapping_add(bytes),
            ));
        }
    });
}
// SAFETY: pointers/layouts pass unchanged to System; the TLS counter never allocates.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size(), false);
        // SAFETY: preserves the caller's valid layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(layout.size(), false);
        // SAFETY: preserves the caller's valid layout.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record(size, true);
        // SAFETY: preserves the caller's pointer, original layout and new size.
        unsafe { System.realloc(pointer, layout, size) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: forwards the original pointer/layout to their allocator.
        unsafe { System.dealloc(pointer, layout) }
    }
}
pub(super) fn measure<T>(operation: impl FnOnce() -> T) -> (T, usize, usize, usize) {
    COUNTERS.with(|counter| counter.set((true, 0, 0, 0)));
    let result = operation();
    let (_, allocations, reallocations, requested) = COUNTERS.with(|counter| {
        let values = counter.get();
        counter.set((false, 0, 0, 0));
        values
    });
    (result, allocations, reallocations, requested)
}
