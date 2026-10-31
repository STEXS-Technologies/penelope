//! Public replay allocation traffic, counted separately from latency.
#![allow(unsafe_code, clippy::unwrap_used, clippy::arithmetic_side_effects)]
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
#[path = "support/process_replay_fixture.rs"]
mod fixture;
use fixture::{Clock, Error, block_on, populate, verify};
use penelope_domain::*;
use penelope_ports::*;
use penelope_sqlite::{SqliteLimits, SqliteProcessStore};
use std::{num::NonZeroU16, sync::Arc};
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
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("allocations.db");
    let store = SqliteProcessStore::open(&path, SqliteLimits::default(), Arc::new(Clock), 10, 16)?;
    let scope = ProcessScope::new(
        "tnt_bench".parse()?,
        "prc_bench".parse()?,
        "def_bench".parse()?,
        "dfv_one".parse()?,
        ContentDigest([1; 32]),
    );
    let history = populate(&store, &scope, 128)?;
    for (profile, from, limit) in [
        ("first", 0, 1),
        ("last", 127, 1),
        ("end", 128, 1),
        ("past", 145, 1),
        ("page", 0, 128),
    ] {
        let request = OutcomeReplayRequest::new(
            scope.clone(),
            from,
            NonZeroU16::new(limit).ok_or("zero page")?,
        )?;
        let mut copied_bytes = 0_usize;
        for outcome in history
            .iter()
            .filter(|outcome| outcome.sequence >= from)
            .take(usize::from(limit))
        {
            copied_bytes = copied_bytes
                .checked_add(serde_json::to_vec(outcome)?.len())
                .and_then(|bytes| bytes.checked_add(32))
                .ok_or("byte count overflow")?;
        }
        for _ in 0..100 {
            verify(
                &block_on(store.read_outcomes(&request))??,
                &request,
                &history,
            )?;
        }
        for sample in 0..16 {
            ALLOCATIONS.store(0, Ordering::SeqCst);
            REALLOCATIONS.store(0, Ordering::SeqCst);
            REQUESTED_BYTES.store(0, Ordering::SeqCst);
            ENABLED.store(true, Ordering::SeqCst);
            let result = block_on(store.read_outcomes(&request));
            ENABLED.store(false, Ordering::SeqCst);
            let allocations = ALLOCATIONS.load(Ordering::SeqCst);
            let reallocations = REALLOCATIONS.load(Ordering::SeqCst);
            let requested_bytes = REQUESTED_BYTES.load(Ordering::SeqCst);
            let page = result??;
            verify(&page, &request, &history)?;
            println!(
                "{}",
                serde_json::json!({"profile":profile,"from":from,"limit":limit,"sample":sample,"rows":page.outcomes.len(),"copied_bytes":copied_bytes,"allocations":allocations,"reallocations":reallocations,"requested_bytes":requested_bytes})
            );
        }
    }
    Ok(())
}
